import { describe, expect, it, vi } from 'vitest';
import type { DashboardRoute } from '../lib/models/types';
import { navigationInbox } from '../lib/utils/navigationInbox';

describe('dashboard navigation inbox', () => {
  it('handles cold, warm, already-focused and repeated requests without resetting the route', async () => {
    let pending: DashboardRoute | null = 'battery';
    const take = vi.fn(async () => { const route = pending; pending = null; return route; });
    const select = vi.fn();
    const inbox = navigationInbox(take, select);
    await inbox.wake();
    expect(take).not.toHaveBeenCalled();
    await inbox.activate();
    for (const route of ['cpu', 'memory', 'disks', 'settings', 'battery'] as DashboardRoute[]) {
      pending = route;
      await Promise.all([inbox.wake(), inbox.wake()]);
    }
    await inbox.wake(); // opening with no destination preserves the current tab
    expect(select.mock.calls.map(([route]) => route)).toEqual(['battery', 'cpu', 'memory', 'disks', 'settings', 'battery']);
    inbox.dispose(); pending = 'memory';
    await inbox.wake();
    expect(pending).toBe('memory');
  });

  it('does not navigate after disposal while a mailbox read is pending', async () => {
    let resolve!: (route: DashboardRoute) => void;
    const select = vi.fn();
    const inbox = navigationInbox(() => new Promise(r => { resolve = r; }), select);
    const active = inbox.activate();
    await Promise.resolve();
    inbox.dispose(); resolve('memory'); await active;
    expect(select).not.toHaveBeenCalled();
  });
});
