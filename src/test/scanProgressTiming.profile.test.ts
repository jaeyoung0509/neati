import { expect, it, vi } from 'vitest';
import { ScanStore } from '../lib/stores/scan.svelte';
import type { ScanEvent, ScanResult } from '../lib/models/types';
import { tauriCancelScan, tauriScan, tauriScanDiscovery } from '../lib/utils/tauri';

vi.mock('../lib/utils/tauri', async (importOriginal) => ({
  ...await importOriginal<typeof import('../lib/utils/tauri')>(),
  tauriScan: vi.fn(), tauriCancelScan: vi.fn(), tauriScanDiscovery: vi.fn(),
}));

it('profiles representative progress bursts while keeping Stop and inventory separate', async () => {
  vi.mocked(tauriScanDiscovery).mockReturnValue({ status: 'exhausted' });
  vi.mocked(tauriCancelScan).mockResolvedValue(undefined);
  for (let iteration = 0; iteration < 6; iteration++) {
    let emit!: (event: ScanEvent) => void;
    let finish!: (result: ScanResult) => void;
    vi.mocked(tauriScan).mockImplementation((callback) => {
      emit = callback;
      return new Promise(resolve => { finish = resolve; });
    });
    const store = new ScanStore();
    const running = store.runScan();
    emit({ type: 'Started', scan_id: 'profile' });
    emit({ type: 'CategoryStarted', category: 'system' });
    const item = {
      id: 'fixture', signature_id: 'fixture', name: 'Fixture', category: 'system',
      risk: 'manual', path: '/fixture', size: { logical: 10, allocated: 10 },
      file_count: 1, description: '', is_selected: false, last_modified: null, exists: true,
      quality: 'fresh', incomplete_reason: null,
      disposition: { eligibility: 'advisory', reason: null, cleanable_bytes: null },
    } as const;
    const started = performance.now();
    for (let index = 0; index < 1092; index++) {
      emit({ type: 'RootStarted', category: 'system', signature_id: 'fixture',
        name: 'Measured root', root: `/fixture/${index}` });
      if (index < 466) emit({ type: 'ItemFound', item });
    }
    const burstMs = performance.now() - started;
    expect(store.foundItemCount).toBe(466);
    expect(store.currentRoot?.path).toBe('/fixture/1091');
    expect(store.lastScan).toBeNull();
    expect(store.canClean).toBe(false);
    const stopStarted = performance.now();
    await store.cancelScan();
    const stopDispatchMs = performance.now() - stopStarted;
    expect(tauriCancelScan).toHaveBeenLastCalledWith('profile');
    const result = {
      scan_id: 'profile', valid_for_seconds: 300, started_at: 1, finished_at: 2,
      total_bytes: 0, safe_bytes: 0, rebuild_bytes: 0, manual_bytes: 0,
      quality: 'partial', incomplete_reasons: [], gaps: [], categories: [], cancelled: true,
    } as ScanResult;
    emit({ type: 'Finished', result });
    finish(result);
    await running;
    expect(store.isScanning).toBe(false);
    console.log(JSON.stringify({ scope: 'Svelte store callbacks; mocked IPC; no DOM rendering',
      iteration, warmup: iteration === 0, events: 1558, burstMs, stopDispatchMs }));
  }
});
