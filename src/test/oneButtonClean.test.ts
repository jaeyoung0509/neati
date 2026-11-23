import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import OneButtonResultModal from '../lib/components/OneButtonResultModal.svelte';
import { OneButtonCleanStore } from '../lib/stores/oneButtonClean.svelte';
import { describeOneButtonActivity, oneButtonOutcomeState } from '../lib/utils/oneButtonClean';
import * as tauri from '../lib/utils/tauri';
import type { OneButtonPreview } from '../lib/models/types';

const allowed = () => null;

function preview(id: string, bytes = 2048): OneButtonPreview {
  return {
    id,
    items: [{ reference: 'system.npm.cache', source: 'cache', name: 'npm cache', kind: 'developer', path: '/Users/me/.npm/_cacache', bytes }],
    total_bytes: bytes,
    cache_bytes: bytes,
    project_bytes: 0,
    review_items: 0,
    review_bytes: 0,
    project_folders: 0,
  };
}

describe('one-button Clean through the backend use case (#500)', () => {
  it('shows the backend preview, including build output, with its activity', async () => {
    const store = new OneButtonCleanStore(allowed);
    await store.refresh();
    expect(store.phase).toBe('ready');
    expect(store.preview?.project_bytes).toBeGreaterThan(0);
    expect(store.totalBytes).toBe(store.preview?.total_bytes);
    expect(store.itemCount).toBe(store.preview?.items.length);
  });

  it('submits only the preview id, once, and drops it before the work starts', async () => {
    const execute = vi.spyOn(tauri, 'tauriExecuteOneButtonClean').mockResolvedValue({
      reclaimed_bytes: 2048, removed: 1, skipped: 0, failed: [],
    });
    const store = new OneButtonCleanStore(allowed);
    store.preview = preview('00000000-0000-4000-8000-0000000000aa');
    store.phase = 'ready';
    const [first, second] = await Promise.all([store.clean(), store.clean()]);
    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute.mock.calls[0][0]).toBe('00000000-0000-4000-8000-0000000000aa');
    expect(first?.removed).toBe(1);
    expect(second).toBeNull();
    expect(store.preview).toBeNull();
    execute.mockRestore();
  });

  it('never deletes from the browser preview', async () => {
    const execute = vi.spyOn(tauri, 'tauriExecuteOneButtonClean');
    const store = new OneButtonCleanStore();
    store.preview = preview('00000000-0000-4000-8000-0000000000bb');
    store.phase = 'ready';
    expect(await store.clean()).toBeNull();
    expect(execute).not.toHaveBeenCalled();
    expect(store.error).toBeTruthy();
    execute.mockRestore();
  });

  it('reports a refused execution without keeping the spent id', async () => {
    const execute = vi.spyOn(tauri, 'tauriExecuteOneButtonClean').mockRejectedValue(
      new Error('This preview is no longer current. Check again before cleaning.')
    );
    const store = new OneButtonCleanStore(allowed);
    store.preview = preview('00000000-0000-4000-8000-0000000000cc');
    store.phase = 'ready';
    expect(await store.clean()).toBeNull();
    expect(store.phase).toBe('error');
    expect(store.error).toContain('no longer current');
    expect(store.preview).toBeNull();
    execute.mockRestore();
  });

  it('names the current step and the result honestly', () => {
    expect(describeOneButtonActivity({ type: 'removing', name: 'npm cache', index: 2, total: 5 }))
      .toBe('Removing npm cache · 3 of 5');
    expect(describeOneButtonActivity({ type: 'measuring', project: 'app', kind: 'cargo target' }))
      .toBe('Measuring app · cargo target…');
    expect(oneButtonOutcomeState({ reclaimed_bytes: 1, removed: 1, skipped: 0, failed: [] })).toBe('success');
    const failure = { reference: 'r', name: 'n', message: 'In use' };
    expect(oneButtonOutcomeState({ reclaimed_bytes: 1, removed: 1, skipped: 0, failed: [failure] })).toBe('partial');
    expect(oneButtonOutcomeState({ reclaimed_bytes: 0, removed: 0, skipped: 0, failed: [failure] })).toBe('failed');

    const body = render(OneButtonResultModal, {
      props: { outcome: { reclaimed_bytes: 3 * 1024 * 1024, removed: 2, skipped: 1, failed: [failure] }, onClose: () => {} },
    }).body;
    expect(body).toContain('Some items could not be cleaned');
    expect(body).toContain('2 items permanently removed · 1 already gone');
    expect(body).toContain('3 MB');
    expect(body).toContain('In use');
  });
});
