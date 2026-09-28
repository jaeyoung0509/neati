import type { DashboardRoute } from '../models/types';

/** A wake-up carries no route; the backend mailbox remains authoritative. */
export function navigationInbox(
  take: () => Promise<DashboardRoute | null>,
  select: (route: DashboardRoute) => void
) {
  let ready = false;
  let disposed = false;
  let queue = Promise.resolve();
  const wake = () => {
    if (!ready || disposed) return queue;
    queue = queue.then(async () => {
      if (disposed) return;
      const route = await take().catch(() => null);
      if (!disposed && route) select(route);
    });
    return queue;
  };
  return {
    wake,
    activate() { ready = true; return wake(); },
    dispose() { disposed = true; },
  };
}
