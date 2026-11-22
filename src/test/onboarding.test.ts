import { describe, expect, it, vi } from 'vitest';
import { LaunchAtLoginStore } from '../lib/stores/launchAtLogin.svelte';
import { ONBOARDING_STEPS } from '../lib/utils/onboarding';
import type { LaunchAtLogin } from '../lib/models/types';

describe('onboarding copy', () => {
  it('names permanent removal and the absence of Trash in the safety step', () => {
    const safety = ONBOARDING_STEPS.find((step) => step.id === 'safety');
    const text = [safety?.title, ...(safety?.paragraphs ?? []), ...(safety?.points ?? [])].join(' ');
    expect(text).toMatch(/permanent/i);
    expect(text).toMatch(/do not go to the Trash/);
  });

  it('has three steps in a fixed order', () => {
    expect(ONBOARDING_STEPS.map((step) => step.id)).toEqual(['what', 'safety', 'access']);
  });
});

describe('LaunchAtLoginStore', () => {
  it('shows the state the system reports, not a saved preference', async () => {
    const store = new LaunchAtLoginStore(async () => ({ status: 'enabled' }), vi.fn());
    expect(store.enabled).toBe(false);
    await store.load();
    expect(store.enabled).toBe(true);
  });

  it('keeps the previous state and surfaces the reason when a change fails', async () => {
    const store = new LaunchAtLoginStore(
      async () => ({ status: 'disabled' }),
      async () => { throw new Error('not a file neati created'); },
    );
    await store.load();
    await store.set(true);
    expect(store.enabled).toBe(false);
    expect(store.error).toContain('not a file neati created');
    expect(store.busy).toBe(false);
  });

  it('never writes when this copy cannot be registered', async () => {
    const write = vi.fn(async (): Promise<LaunchAtLogin> => ({ status: 'enabled' }));
    const store = new LaunchAtLoginStore(async () => ({ status: 'unavailable', reason: 'Move neati to Applications' }), write);
    await store.load();
    await store.set(true);
    expect(write).not.toHaveBeenCalled();
    expect(store.unavailableReason).toBe('Move neati to Applications');
  });

  it('reports a failed read as unavailable instead of off', async () => {
    const store = new LaunchAtLoginStore(async () => { throw new Error('ipc'); }, vi.fn());
    await store.load();
    expect(store.unavailableReason).toContain('ipc');
  });
});
