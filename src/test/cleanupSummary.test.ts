import { describe, expect, it } from 'vitest';
import { cleanupSummaryState } from '../lib/utils/cleanupSummary';

const base = {
  available: true,
  hasScan: true,
  scanning: false,
  cleaning: false,
  freshness: 'fresh' as const,
  discovery: { status: 'exhausted' as const },
  cleanableBytes: 0,
};

describe('shared cleanup summary phase', () => {
  it('reserves zero for a fresh completed inventory', () => {
    expect(cleanupSummaryState(base)).toBe('clean');
    expect(cleanupSummaryState({ ...base, freshness: 'stale' })).toBe('stale');
    expect(cleanupSummaryState({ ...base, freshness: 'failed' })).toBe('failed');
    expect(cleanupSummaryState({ ...base, freshness: 'partial' })).toBe('partial');
    expect(cleanupSummaryState({ ...base, hasScan: false, freshness: 'empty' })).toBe('unknown');
  });

  it('keeps cleaning, scanning, and post-clean measurement distinct', () => {
    expect(cleanupSummaryState({ ...base, cleaning: true })).toBe('cleaning');
    expect(cleanupSummaryState({ ...base, scanning: true })).toBe('refreshing');
    expect(cleanupSummaryState({ ...base, scanning: true, hasScan: false })).toBe('scanning');
    expect(cleanupSummaryState({ ...base, cleanableBytes: 4096 })).toBe('ready');
    expect(cleanupSummaryState({ ...base, available: false })).toBe('unavailable');
  });

  it('never presents retained paused or stopped estimates as ready, even with current measured bytes', () => {
    const measured = { ...base, cleanableBytes: 376 * 1024, freshness: 'partial' as const };
    expect(cleanupSummaryState({ ...measured, discovery: { status: 'stopped', reason: 'Cancelled' } })).toBe('stopped');
    expect(cleanupSummaryState({ ...measured, discovery: { status: 'paused', continuation_id: 'fixture' } })).toBe('paused');
    expect(cleanupSummaryState(measured)).toBe('partial');
  });
});
