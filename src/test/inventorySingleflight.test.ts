import { afterEach, describe, expect, it, vi } from 'vitest';
import { LocalModelsStore } from '../lib/stores/models.svelte';
import { DockerStore } from '../lib/stores/docker.svelte';
import { DevelopmentPortsStore } from '../lib/stores/developmentPorts.svelte';
import * as tauri from '../lib/utils/tauri';
import type { DockerStatus, LocalModelInventory } from '../lib/models/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

const models: LocalModelInventory = {
  items: [], quality: 'fresh', skipped_entry_count: 0, incomplete_reasons: [],
};
const docker: DockerStatus = {
  is_available: true, is_running: true, version: 'fixture', error_message: null,
  overview: null, images: [], containers: [], volumes: [],
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe('concurrent inventory readers', () => {
  it('shares an in-flight model traversal and requests again after it completes', async () => {
    const first = deferred<LocalModelInventory>();
    const read = vi.spyOn(tauri, 'tauriGetLocalModels')
      .mockReturnValueOnce(first.promise).mockResolvedValue(models);
    const store = new LocalModelsStore();
    const a = store.refresh();
    const b = store.refresh();
    expect(read).toHaveBeenCalledTimes(1);
    expect(store.isLoading).toBe(true);
    first.resolve(models);
    await Promise.all([a, b]);
    expect(store.isLoading).toBe(false);
    await store.refresh();
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('shares a simultaneous container status request without caching later reads', async () => {
    const first = deferred<DockerStatus>();
    const read = vi.spyOn(tauri, 'tauriGetDockerStatus')
      .mockReturnValueOnce(first.promise).mockResolvedValue(docker);
    const store = new DockerStore();
    const a = store.refresh();
    const b = store.refresh();
    expect(read).toHaveBeenCalledTimes(1);
    first.resolve(docker);
    await Promise.all([a, b]);
    await store.refresh();
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('does a new model read after a mutation even if an older traversal is still pending', async () => {
    const old = deferred<LocalModelInventory>();
    const read = vi.spyOn(tauri, 'tauriGetLocalModels')
      .mockReturnValueOnce(old.promise)
      .mockResolvedValue({ ...models, quality: 'partial', incomplete_reasons: ['new fixture'] });
    const store = new LocalModelsStore();
    const previous = store.refresh();
    const afterMutation = store.refresh(true);
    expect(read).toHaveBeenCalledTimes(1);
    old.resolve(models);
    await Promise.all([previous, afterMutation]);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.quality).toBe('partial');
    expect(store.incompleteReasons).toEqual(['new fixture']);
  });

  it('retains a failed container read and allows a later retry', async () => {
    const read = vi.spyOn(tauri, 'tauriGetDockerStatus')
      .mockRejectedValueOnce(new Error('fixture unavailable')).mockResolvedValue(docker);
    const store = new DockerStore();
    await Promise.all([store.refresh(), store.refresh()]);
    expect(read).toHaveBeenCalledTimes(1);
    expect(store.error).toContain('fixture unavailable');
    expect(store.isLoading).toBe(false);
    await store.refresh();
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.error).toBeNull();
  });

  it('releases only its own development-listener polling subscription', async () => {
    vi.useFakeTimers();
    const read = vi.spyOn(tauri, 'tauriListDevelopmentListeners').mockResolvedValue([]);
    const store = new DevelopmentPortsStore();
    expect(vi.getTimerCount()).toBe(0);
    const releaseA = store.observePolling(1000);
    const releaseB = store.observePolling(1000);
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(1);
    releaseA();
    releaseA();
    await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(2);
    releaseB();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(5000);
    expect(read).toHaveBeenCalledTimes(2);
  });
});
