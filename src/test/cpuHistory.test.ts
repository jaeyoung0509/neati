import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import MetricSparkline from '../lib/components/metrics/MetricSparkline.svelte';
import { SystemMetricsStore } from '../lib/stores/systemMetrics.svelte';
import { CPU_HISTORY_WINDOW_MS, recentCpuSamples } from '../lib/utils/cpuHistory';
import type { CpuMetrics } from '../lib/models/types';

afterEach(() => vi.useRealTimers());

function reading(at: number, percent = 25): CpuMetrics {
  return { state: 'fresh', usage_percent: percent, sample_interval_ms: 2500,
    sampled_at: at, stale_after_ms: 3000, cores: 8, reason: null };
}
function chart(samples: {at: number; percent: number}[], endAt = 150_000) {
  return render(MetricSparkline, { props: { samples, endAt } }).body;
}
function traces(body: string) {
  return [...body.matchAll(/<polyline[^>]*points="([^"]+)"/g)].map(match => match[1]);
}

describe('CPU history time window and geometry', () => {
  it('retains elapsed-time coverage and a hard cap without filling missing readings', () => {
    expect(recentCpuSamples([
      { at: 1, percent: 50 }, { at: 150_000, percent: 10 }, { at: 300_000, percent: 20 },
    ], 300_000)).toEqual([{ at: 150_000, percent: 10 }, { at: 300_000, percent: 20 }]);
    expect(recentCpuSamples(Array.from({ length: 100 }, (_, i) => ({ at: 200_000 + i, percent: 20 })), 300_000)).toHaveLength(60);
    expect(recentCpuSamples([{ at: 300_001, percent: 10 }, { at: 300_000, percent: NaN }], 300_000)).toEqual([]);
  });

  it('keeps a fixed time scale rather than stretching two readings across the whole plot', () => {
    const body = chart([{ at: 147_500, percent: 0 }, { at: 150_000, percent: 100 }]);
    expect(traces(body)).toEqual(['97.367,108.000 99.000,4.000']);
    expect(body).toContain('2m 30s ago');
    expect(body).toContain('100%');
    expect(body).toContain('50%');
    expect(body).toContain('0%');
    expect(body).toContain('Most recent: 100.0 percent');
  });

  it('separates actual recording gaps and removes expired groups', () => {
    const body = chart([
      { at: -100_000, percent: 15 },
      { at: 100_000, percent: 10 }, { at: 102_500, percent: 20 },
      { at: 147_500, percent: 30 }, { at: 150_000, percent: 40 },
    ]);
    expect(traces(body)).toHaveLength(2);
    expect(body).toContain('4 recorded samples');
    expect(body).toContain('1 recording gap;');
  });

  it('draws one observation as a non-scaling round point, without a fabricated line', () => {
    const body = chart([{ at: 150_000, percent: 20 }]);
    expect(traces(body)).toEqual([]);
    expect(body).not.toContain('<circle');
    expect(body).toMatch(/<line[^>]*stroke-width="5"[^>]*stroke-linecap="round"[^>]*vector-effect="non-scaling-stroke"/);
    expect(body).toContain('1 recorded sample');
  });

  it('names empty history and never turns it into a zero trace', () => {
    const body = chart([]);
    expect(body).toContain('No readings in this window');
    expect(body).toContain('No CPU readings recorded');
    expect(traces(body)).toEqual([]);
    expect(body).not.toContain('stroke-width="5"');
  });
});

describe('CPU sampling around a foreground pause', () => {
  it('expires old clusters on reopening and keeps one subscriber-owned collector', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(300_000);
    const probe = vi.fn(async () => reading(Date.now()));
    const store = new SystemMetricsStore(probe, vi.fn(async () => { throw new Error('No battery'); }));
    const release = store.observePolling();
    await vi.advanceTimersByTimeAsync(7500);
    expect(store.cpuHistory).toHaveLength(4);
    release();
    await vi.advanceTimersByTimeAsync(CPU_HISTORY_WINDOW_MS * 2);
    expect(probe).toHaveBeenCalledTimes(4);
    const releaseReopened = store.observePolling();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.cpuHistory).toEqual([{ at: Date.now(), percent: 25 }]);
    expect(store.cpuHistoryEndAt).toBe(Date.now());
    releaseReopened();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('ages out history on a failed refresh and labels the retained reading as failed', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(300_000);
    const probe = vi.fn().mockResolvedValueOnce(reading(Date.now())).mockRejectedValue(new Error('CPU probe failed'));
    const store = new SystemMetricsStore(probe);
    await store.refreshCpu();
    vi.setSystemTime(500_000);
    await store.refreshCpu();
    expect(store.cpuHistory).toEqual([]);
    expect(store.cpu?.state).toBe('failed');
    expect(store.cpu?.usage_percent).toBe(25);
    expect(store.cpuError).toBe('CPU probe failed');
    expect(store.cpuHistoryEndAt).toBe(500_000);
  });

  it('does not let duplicate or out-of-order responses add a backward trace', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(300_000);
    const probe = vi.fn().mockResolvedValueOnce(reading(300_000)).mockResolvedValueOnce(reading(300_000)).mockResolvedValueOnce(reading(299_000));
    const store = new SystemMetricsStore(probe);
    await store.refreshCpu(); await store.refreshCpu(); await store.refreshCpu();
    expect(store.cpuHistory).toEqual([{ at: 300_000, percent: 25 }]);
  });
});
