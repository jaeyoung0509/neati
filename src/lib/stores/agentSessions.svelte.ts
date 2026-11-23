import { describeError } from '../utils/errors';
import { loadFailed, loadStarted, type LoadPhase } from '../utils/loadPhase';
import type { AgentSessionInventory } from '../models/types';
import { tauriCancelSessionOperation, tauriGetAgentSessions } from '../utils/tauri';

/**
 * The Sessions view's state (#508). The inventory is read-only: the window
 * asks for it and can stop a running read, nothing more.
 */
export class AgentSessionsStore {
  inventory = $state<AgentSessionInventory | null>(null);
  phase = $state<LoadPhase>('idle');
  /** A read is running, the first or a refresh. */
  reading = $state(false);
  error = $state<string | null>(null);

  private loading: Promise<void> | null = null;

  load(): Promise<void> {
    if (this.loading) return this.loading;
    const pending = this.read();
    this.loading = pending;
    void pending.finally(() => {
      if (this.loading === pending) this.loading = null;
    });
    return pending;
  }

  private async read(): Promise<void> {
    this.phase = loadStarted(this.phase);
    this.reading = true;
    this.error = null;
    try {
      this.inventory = await tauriGetAgentSessions();
      this.phase = 'ready';
    } catch (error) {
      this.error = describeError(error, 'Agent sessions could not be read.');
      this.phase = loadFailed(this.phase);
    } finally {
      this.reading = false;
    }
  }

  /** Stops the running read. */
  async stop(): Promise<void> {
    await tauriCancelSessionOperation();
  }
}

export const agentSessionsStore = new AgentSessionsStore();
