import { describe, expect, it, vi } from 'vitest';
import { UpdateStore } from '../lib/stores/updates.svelte';
import { updateStatusCopy } from '../lib/utils/updateCopy';
import type { UpdateCheck } from '../lib/models/types';

const available: UpdateCheck = {
  status: 'available',
  current_version: '0.3.110',
  version: '0.3.111',
  notes: 'Fixes',
  published_at: '2026-11-22T00:00:00Z',
};

describe('UpdateStore', () => {
  it('makes no request at launch unless the person opted in', async () => {
    const check = vi.fn(async () => available);
    const store = new UpdateStore(check, vi.fn());
    await store.checkOnLaunch(false);
    expect(check).not.toHaveBeenCalled();
  });

  it('runs the opted-in launch check once per window', async () => {
    const check = vi.fn(async () => available);
    const store = new UpdateStore(check, vi.fn());
    await store.checkOnLaunch(true);
    await store.checkOnLaunch(true);
    expect(check).toHaveBeenCalledOnce();
    expect(store.available?.version).toBe('0.3.111');
  });

  it('installs only the version it was shown', async () => {
    const install = vi.fn(async () => {});
    const store = new UpdateStore(async () => available, install);
    await store.installAvailable();
    expect(install).not.toHaveBeenCalled();
    await store.checkNow();
    await store.installAvailable();
    expect(install).toHaveBeenCalledWith('0.3.111');
  });

  it('keeps a failed install visible and returns to idle', async () => {
    const store = new UpdateStore(async () => available, async () => { throw new Error('signature mismatch'); });
    await store.checkNow();
    await store.installAvailable();
    expect(store.error).toContain('signature mismatch');
    expect(store.phase).toBe('idle');
  });

  it('reports a thrown check as a failed result instead of a stale one', async () => {
    const store = new UpdateStore(async () => { throw new Error('offline'); }, vi.fn());
    await store.checkNow();
    expect(store.result).toEqual({ status: 'failed', message: 'Error: offline' });
  });
});

describe('updateStatusCopy', () => {
  it('words each backend outcome without inventing reasons', () => {
    expect(updateStatusCopy(null)).toBe('');
    expect(updateStatusCopy({ status: 'up_to_date', current_version: '0.3.110' })).toContain('newest');
    expect(updateStatusCopy(available)).toContain('0.3.111');
    expect(updateStatusCopy({ status: 'unavailable', reason: 'No key' })).toBe('No key');
    expect(updateStatusCopy({ status: 'failed', message: 'Offline' })).toBe('Offline');
  });
});
