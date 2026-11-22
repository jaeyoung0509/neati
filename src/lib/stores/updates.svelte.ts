import type { UpdateCheck } from '../models/types';
import { tauriCheckForUpdate, tauriInstallUpdate } from '../utils/tauri';

export type UpdatePhase = 'idle' | 'checking' | 'installing';

/**
 * Update state for the main window. Every check is started by the person or by
 * their opt-in launch check; nothing here runs on a timer. The backend owns
 * which checked update may be installed, so this store only remembers what it
 * was told and passes the shown version back.
 */
export class UpdateStore {
  phase = $state<UpdatePhase>('idle');
  result = $state<UpdateCheck | null>(null);
  error = $state('');
  private launchCheckStarted = false;

  constructor(
    private readonly check = tauriCheckForUpdate,
    private readonly install = tauriInstallUpdate,
  ) {}

  get available(): Extract<UpdateCheck, { status: 'available' }> | null {
    return this.result?.status === 'available' ? this.result : null;
  }

  async checkNow(): Promise<void> {
    if (this.phase !== 'idle') return;
    this.phase = 'checking';
    this.error = '';
    try {
      this.result = await this.check();
    } catch (cause) {
      this.result = { status: 'failed', message: String(cause) };
    } finally {
      this.phase = 'idle';
    }
  }

  /** Runs at most once per window lifetime, and only when the person opted in. */
  async checkOnLaunch(enabled: boolean): Promise<void> {
    if (!enabled || this.launchCheckStarted) return;
    this.launchCheckStarted = true;
    await this.checkNow();
  }

  /** On success the app restarts, so only failure returns here. */
  async installAvailable(): Promise<void> {
    const update = this.available;
    if (!update || this.phase !== 'idle') return;
    this.phase = 'installing';
    this.error = '';
    try {
      await this.install(update.version);
    } catch (cause) {
      this.error = String(cause);
    } finally {
      this.phase = 'idle';
    }
  }
}

export const updateStore = new UpdateStore();
