import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { quitAndRescan } from '../lib/utils/cleanupQuit';
import type { CleanupQuitPreview, ScanResult, ScanItem } from '../lib/models/types';
beforeEach(() => vi.stubGlobal('window', { __TAURI_INTERNALS__: {} }));
afterEach(() => vi.unstubAllGlobals());
const preview: CleanupQuitPreview = { scan_id: 'old', unavailable: [], apps: [
  { name: 'Chrome', lease_id: 'opaque', item_ids: ['cache'] },
  { name: 'Editor', lease_id: 'opaque2', item_ids: ['editor-cache'] },
] };
const row = (id: string, running = false): ScanItem => ({ id, risk: 'rebuild', exists: true,
  owner_running: running, size: { logical: 10, allocated: 10 }, disposition: { eligibility: running ? 'reviewable' : 'auto_cleanable', cleanable_bytes: 10 },
} as ScanItem);
const scan = { scan_id: 'fresh', cancelled: false, categories: [{ items: [row('cache'), row('editor-cache', true), row('unreviewed')] }] } as ScanResult;
describe('quit, rescan and review', () => {
  it('refuses preview mode before dispatching a quit request', async () => {
    vi.stubGlobal('window', {});
    const quit = vi.fn();
    const rescan = vi.fn();
    await expect(quitAndRescan(preview, { quit, scan: rescan, cancelled: () => false, progress: () => {} })).rejects.toThrow('preview data');
    expect(quit).not.toHaveBeenCalled();
    expect(rescan).not.toHaveBeenCalled();
  });
  it('returns only reviewed identities still executable in a fresh scan', async () => {
    const quit = vi.fn().mockResolvedValue({ outcome: 'released' });
    const rescan = vi.fn().mockResolvedValue(scan);
    const items = await quitAndRescan(preview, { quit, scan: rescan, cancelled: () => false, progress: () => {} });
    expect(quit.mock.calls).toEqual([['opaque'], ['opaque2']]);
    expect(rescan).toHaveBeenCalledOnce();
    expect(items.map(item => item.id)).toEqual(['cache']);
  });
  it('never force quits or starts scanning after a failed graceful request', async () => {
    const quit = vi.fn().mockResolvedValue({ outcome: 'still_listening' });
    const rescan = vi.fn();
    await expect(quitAndRescan(preview, { quit, scan: rescan, cancelled: () => false, progress: () => {} })).rejects.toThrow('Chrome');
    expect(quit).toHaveBeenCalledOnce(); expect(rescan).not.toHaveBeenCalled();
  });
  it('stops between apps when cancelled and never starts a rescan', async () => {
    let cancelled = false;
    const quit = vi.fn().mockImplementation(async () => { cancelled = true; return { outcome: 'released' }; });
    const rescan = vi.fn();
    expect(await quitAndRescan(preview, { quit, scan: rescan, cancelled: () => cancelled, progress: () => {} })).toEqual([]);
    expect(quit).toHaveBeenCalledOnce(); expect(rescan).not.toHaveBeenCalled();
  });
  it('never offers cleanup from an interrupted rescan', async () => {
    const quit = vi.fn().mockResolvedValue({ outcome: 'released' });
    expect(await quitAndRescan(preview, { quit, scan: async () => ({ ...scan, cancelled: true }), cancelled: () => false, progress: () => {} })).toEqual([]);
  });
});
