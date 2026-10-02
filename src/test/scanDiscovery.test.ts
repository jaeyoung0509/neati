import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { nativeApi } from '../lib/api/native';
import { mockApi } from '../lib/api/mock';
import { ScanStore } from '../lib/stores/scan.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import type { PublishedScan } from '../lib/models/types';
import { retainedScanFixture } from './fixtures/retainedScan';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('native publication completion and retained selection', () => {
  it('retains nonempty AutoCleanable rows after Stop without restoring cleanup authority', async () => {
    const publication = retainedScanFixture({ status: 'stopped', reason: 'Scan was cancelled.' });
    let finish!: (published: PublishedScan) => void;
    vi.spyOn(nativeApi, 'startScan').mockImplementation(async onEvent => {
      onEvent({ type: 'Started', scan_id: publication.result.scan_id });
      for (const item of publication.result.categories[0].items) onEvent({ type: 'ItemFound', item });
      return new Promise(resolve => { finish = resolve; });
    });
    const cancel = vi.spyOn(nativeApi, 'cancelScan').mockResolvedValue(undefined);
    const plan = vi.spyOn(nativeApi, 'createPlan');
    const execute = vi.spyOn(nativeApi, 'executeClean');
    const store = new ScanStore();
    const running = store.runScan();
    await store.cancelScan();
    expect(cancel).toHaveBeenCalledWith(publication.result.scan_id);
    expect(store.isCancelling).toBe(true);
    finish(publication);
    await running;

    // This exercises the real tauriScan publication adapter, rather than a
    // result-only mock that silently labels a stopped publication exhausted.
    expect(store.discovery).toEqual(publication.discovery);
    expect(store.lastScan).toEqual(publication.result);
    expect(store.lastScan?.categories[0].items).toHaveLength(3);
    expect(store.lastScan?.cleanable_bytes).toBe(376 * 1024);
    expect(store.freshness).toBe('partial');
    expect(store.error).toBeNull();
    expect(store.isScanning).toBe(false);
    expect(store.isCancelling).toBe(false);
    expect(store.selectedCount).toBe(0);
    expect(store.reclaimableBytes).toBe(0);
    expect(store.canClean).toBe(false);
    store.setAllSelected(true);
    store.toggleCategory('system', true);
    store.setItemSelected(publication.result.categories[0].items[0].id, true);
    store.selectQuickCleanDefaults(settingsStore.settings);
    expect(store.selectedCount).toBe(0);
    await expect(store.cleanSelected()).resolves.toBeNull();
    expect(plan).not.toHaveBeenCalled();
    expect(execute).not.toHaveBeenCalled();
  });

  it('waits through a nonempty paused snapshot and selects verified rows after partial exhaustion', async () => {
    const paused = retainedScanFixture({ status: 'paused', continuation_id: 'fixture-continuation' });
    const exhausted = retainedScanFixture({ status: 'exhausted' });
    vi.spyOn(nativeApi, 'startScan').mockResolvedValue(paused);
    const resume = vi.spyOn(nativeApi, 'resumeScan').mockResolvedValue(exhausted);
    const create = vi.spyOn(nativeApi, 'createPlan').mockImplementation((scanId, items) => mockApi.createPlan(scanId, items));
    const store = new ScanStore();
    await store.runScan();
    expect(store.lastScan?.total_bytes).toBe(376 * 1024);
    expect(store.canContinue).toBe(true);
    expect(store.canClean).toBe(false);
    expect(store.selectedCount).toBe(0);
    store.selectQuickCleanDefaults(settingsStore.settings);
    expect(store.selectedCount).toBe(0);
    await expect(store.prepareCleanup(paused.result.categories[0].items)).resolves.toBeNull();
    expect(create).not.toHaveBeenCalled();

    await store.continueScan();

    expect(resume).toHaveBeenCalledWith(expect.any(Function), paused.result.scan_id, 'fixture-continuation');
    expect(store.discovery).toEqual({ status: 'exhausted' });
    expect(store.freshness).toBe('partial');
    expect(store.canClean).toBe(true);
    expect(store.canContinue).toBe(false);
    expect(store.selectedCount).toBe(3);
    expect(store.reclaimableBytes).toBe(376 * 1024);
    expect(await store.prepareCleanup(exhausted.result.categories[0].items)).not.toBeNull();
    expect(create).toHaveBeenCalledWith(exhausted.result.scan_id, exhausted.result.categories[0].items);
  });
});
