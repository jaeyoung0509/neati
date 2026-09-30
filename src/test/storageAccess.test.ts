/** @vitest-environment jsdom */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { StorageAccessStore } from '../lib/stores/storageAccess.svelte';

const disposers: (() => void)[] = [];
afterEach(() => {
  disposers.splice(0).forEach((dispose) => dispose());
  vi.restoreAllMocks();
});

function fixture() {
  const ports = {
    openSettings: vi.fn().mockResolvedValue(undefined),
    scan: vi.fn().mockResolvedValue(true),
    busy: vi.fn(() => false),
    native: vi.fn(() => true),
  };
  const store = new StorageAccessStore(ports);
  return { store, ports };
}
function subscribe(store: StorageAccessStore) {
  const dispose = store.subscribe();
  disposers.push(dispose);
  return dispose;
}
function returnToApp() {
  window.dispatchEvent(new Event('blur'));
  window.dispatchEvent(new Event('focus'));
}

describe('storage access recovery', () => {
  it('does no work at construction, ordinary focus or Settings opening alone', async () => {
    const { store, ports } = fixture();
    expect(ports.scan).not.toHaveBeenCalled();
    subscribe(store);
    returnToApp();
    expect(ports.scan).not.toHaveBeenCalled();
    await store.openSettings();
    window.dispatchEvent(new Event('focus'));
    expect(store.phase).toBe('waiting');
    expect(ports.scan).not.toHaveBeenCalled();
  });

  it('runs one fresh scan on a Settings round trip even with repeated focus events', async () => {
    const { store, ports } = fixture();
    subscribe(store);
    await store.openSettings();
    returnToApp();
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(store.phase).toBe('checked'));
    expect(ports.scan).toHaveBeenCalledTimes(1);
    returnToApp();
    expect(ports.scan).toHaveBeenCalledTimes(1);
  });

  it('handles a return before the Settings command resolves', async () => {
    const { store, ports } = fixture();
    let opened!: () => void;
    ports.openSettings.mockReturnValue(new Promise<void>((resolve) => { opened = resolve; }));
    subscribe(store);
    const opening = store.openSettings();
    returnToApp();
    expect(ports.scan).not.toHaveBeenCalled();
    opened();
    await opening;
    await vi.waitFor(() => expect(store.phase).toBe('checked'));
    expect(ports.scan).toHaveBeenCalledTimes(1);
  });

  it('defers a return check while scanning or cleaning and coalesces idle notifications', async () => {
    const { store, ports } = fixture();
    subscribe(store);
    await store.openSettings();
    ports.busy.mockReturnValue(true);
    returnToApp();
    expect(store.phase).toBe('queued');
    store.checkWhenIdle();
    expect(ports.scan).not.toHaveBeenCalled();
    ports.busy.mockReturnValue(false);
    store.checkWhenIdle();
    store.checkWhenIdle();
    await vi.waitFor(() => expect(store.phase).toBe('checked'));
    expect(ports.scan).toHaveBeenCalledTimes(1);
  });

  it('shares event ownership and removes the last subscriber idempotently', async () => {
    const { store, ports } = fixture();
    const first = subscribe(store);
    const second = subscribe(store);
    first();
    first();
    await store.openSettings();
    returnToApp();
    await vi.waitFor(() => expect(store.phase).toBe('checked'));
    expect(ports.scan).toHaveBeenCalledTimes(1);
    await store.openSettings();
    second();
    returnToApp();
    expect(ports.scan).toHaveBeenCalledTimes(1);
    expect(store.phase).toBe('idle');
  });

  it('leaves a queued check pending while hidden and resumes on visibility activation', async () => {
    const { store, ports } = fixture();
    subscribe(store);
    await store.openSettings();
    ports.busy.mockReturnValue(true);
    returnToApp();
    expect(store.phase).toBe('queued');
    const visibility = vi.spyOn(document, 'visibilityState', 'get');
    visibility.mockReturnValue('hidden');
    ports.busy.mockReturnValue(false);
    store.checkWhenIdle();
    window.dispatchEvent(new Event('focus'));
    expect(ports.scan).not.toHaveBeenCalled();
    visibility.mockReturnValue('visible');
    document.dispatchEvent(new Event('visibilitychange'));
    await vi.waitFor(() => expect(store.phase).toBe('checked'));
    expect(ports.scan).toHaveBeenCalledTimes(1);
  });

  it('does not report a completed check after failure and supports a retry', async () => {
    const { store, ports } = fixture();
    ports.scan.mockResolvedValueOnce(false);
    await store.check();
    expect(store.phase).toBe('failed');
    expect(store.error).toContain('could not be rechecked');
    await store.check();
    expect(store.phase).toBe('checked');
    expect(store.error).toBeNull();
  });

  it('shows Settings errors without starting a scan', async () => {
    const { store, ports } = fixture();
    subscribe(store);
    ports.openSettings.mockRejectedValue(new Error('Settings could not be opened'));
    await store.openSettings();
    returnToApp();
    expect(store.phase).toBe('failed');
    expect(store.error).toBe('Settings could not be opened');
    expect(ports.scan).not.toHaveBeenCalled();
  });

  it('ignores late command results after the last surface unmounts', async () => {
    const { store, ports } = fixture();
    let opened!: () => void;
    ports.openSettings.mockReturnValue(new Promise<void>((resolve) => { opened = resolve; }));
    const dispose = subscribe(store);
    const opening = store.openSettings();
    dispose();
    opened();
    await opening;
    expect(store.phase).toBe('idle');
    returnToApp();
    expect(ports.scan).not.toHaveBeenCalled();
  });

  it('refuses preview Settings and permission checks without making native calls', async () => {
    const { store, ports } = fixture();
    ports.native.mockReturnValue(false);
    await store.openSettings();
    expect(store.error).toContain('Browser preview');
    await store.check();
    expect(store.error).toContain('Browser preview');
    expect(ports.openSettings).not.toHaveBeenCalled();
    expect(ports.scan).not.toHaveBeenCalled();
  });
});
