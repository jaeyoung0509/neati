import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import CleanProgressCard from '../lib/components/CleanProgressCard.svelte';
import ElapsedTime from '../lib/components/ElapsedTime.svelte';
import UpdateSettings from '../lib/components/UpdateSettings.svelte';
import { UpdateStore, updateStore } from '../lib/stores/updates.svelte';
import {
  IDLE_CLEAN_PROGRESS,
  cleanPhaseLabel,
  cleanProgressLabel,
  elapsedSeconds,
  formatElapsed,
  reduceCleanProgress,
} from '../lib/utils/cleanProgress';
import { UPDATE_INSTALL_PHASE } from '../lib/utils/updateCopy';
import type { CleanEvent, UpdateCheck } from '../lib/models/types';
import { buttonTag, isDisabled } from './fixtures/markup';

const T0 = 1_800_000_000_000;

// Long removals and installs name their phase and elapsed time (#491).
describe('clean progress phase and elapsed time', () => {
  it('formats elapsed seconds, minutes and hours', () => {
    expect(formatElapsed(0)).toBe('0 s');
    expect(formatElapsed(8.9)).toBe('8 s');
    expect(formatElapsed(125)).toBe('2 min 5 s');
    expect(formatElapsed(3_840)).toBe('1 h 4 min');
    expect(elapsedSeconds(null, T0)).toBe(0);
    expect(elapsedSeconds(T0, T0 - 5_000)).toBe(0);
    expect(elapsedSeconds(T0, T0 + 8_400)).toBe(8);
  });

  it('names the target being removed and its place in the plan', () => {
    expect(cleanPhaseLabel('npm cache', 3, 12)).toBe('Removing npm cache · 3 of 12');
    expect(cleanPhaseLabel('', 0, 12)).toBe('Starting cleanup · 12 items');
    expect(cleanPhaseLabel('', 0, 1)).toBe('Starting cleanup · 1 item');
    expect(cleanPhaseLabel('', 0, 0)).toBe('Starting cleanup');
    expect(cleanProgressLabel({ currentItem: 'npm cache', index: 3, total: 12, percent: 17 }, 8))
      .toBe('Removing npm cache · 3 of 12 · 8 s');
  });

  it('counts a target as done when it finishes, not when it starts', () => {
    const events: CleanEvent[] = [
      { type: 'Started', plan_id: 'plan', total_targets: 2, expected_bytes: 10 },
      { type: 'ItemStarted', item_id: 'a', name: 'npm cache', index: 1, total: 2 },
    ];
    let progress = events.reduce(reduceCleanProgress, { ...IDLE_CLEAN_PROGRESS });
    expect(progress).toEqual({ currentItem: 'npm cache', index: 1, total: 2, percent: 0 });

    progress = reduceCleanProgress(progress, {
      type: 'ItemFinished', item_id: 'a', name: 'npm cache', status: 'success', success: true, reclaimed_bytes: 5, error: null,
    });
    expect(progress.percent).toBe(50);

    progress = reduceCleanProgress(progress, { type: 'ItemStarted', item_id: 'b', name: 'Cargo registry', index: 2, total: 2 });
    // The last (often largest) target is still running: the bar is not full.
    expect(progress).toEqual({ currentItem: 'Cargo registry', index: 2, total: 2, percent: 50 });
    expect(reduceCleanProgress(progress, { type: 'Error', message: 'x' })).toBe(progress);
  });

  describe('rendered surfaces', () => {
    beforeEach(() => {
      vi.useFakeTimers();
      vi.setSystemTime(T0);
    });
    afterEach(() => vi.useRealTimers());

    it('shows elapsed time from the mounted clock', () => {
      vi.setSystemTime(T0 + 12_000);
      expect(render(ElapsedTime, { props: { since: T0 } }).body).toContain('12 s');
      vi.setSystemTime(T0 + 75_000);
      expect(render(ElapsedTime, { props: { since: T0 } }).body).toContain('1 min 15 s');
    });

    it('shows the removal phase and elapsed time in the Storage clean card', () => {
      vi.setSystemTime(T0 + 8_000);
      const body = render(CleanProgressCard, {
        props: { progress: { currentItem: 'npm cache', index: 3, total: 12, percent: 17 }, startedAt: T0 },
      }).body;
      expect(body).toContain('Removing npm cache · 3 of 12');
      expect(body).toContain('8 s');
      expect(body).toContain('role="status"');
      expect(body).not.toContain('Cleaning: ');
    });

    it('says the cleanup is starting before the first target', () => {
      const body = render(CleanProgressCard, {
        props: { progress: { currentItem: '', index: 0, total: 4, percent: 0 }, startedAt: T0 },
      }).body;
      expect(body).toContain('Starting cleanup · 4 items');
    });
  });
});

describe('update install phase and elapsed time', () => {
  const available: UpdateCheck = {
    status: 'available', version: '9.9.9', current_version: '0.3.134', notes: null, published_at: null,
  };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
  });
  afterEach(() => {
    vi.useRealTimers();
    updateStore.phase = 'idle';
    updateStore.phaseStartedAt = null;
    updateStore.result = null;
  });

  it('records when the install started and clears it when it ends', async () => {
    let finish!: () => void;
    const install = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const store = new UpdateStore(vi.fn(async () => available), install);
    store.result = available;
    const pending = store.installAvailable();
    expect(store.phase).toBe('installing');
    expect(store.phaseStartedAt).toBe(T0);
    // A second press while installing does not start another install.
    await store.installAvailable();
    expect(install).toHaveBeenCalledTimes(1);
    finish();
    await pending;
    expect(store.phase).toBe('idle');
    expect(store.phaseStartedAt).toBeNull();
  });

  it('shows the install phase with elapsed seconds next to the busy button', () => {
    updateStore.result = available;
    updateStore.phase = 'installing';
    updateStore.phaseStartedAt = T0;
    vi.setSystemTime(T0 + 8_000);
    const body = render(UpdateSettings).body;
    expect(body).toContain(UPDATE_INSTALL_PHASE);
    expect(body).toContain('8 s');
    const install = buttonTag(body, 'Install and restart');
    expect(isDisabled(install)).toBe(true);
    expect(install).toContain('aria-busy="true"');
    expect(isDisabled(buttonTag(body, 'Check now'))).toBe(true);
  });
});
