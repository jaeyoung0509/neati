import type { LaunchAtLogin } from '../models/types';
import { tauriGetLaunchAtLogin, tauriSetLaunchAtLogin } from '../utils/tauri';

/**
 * What the system holds for "start at login". The switch reflects the
 * registration itself, so a failed change leaves the previous, true state.
 */
export class LaunchAtLoginStore {
  state = $state<LaunchAtLogin | null>(null);
  busy = $state(false);
  error = $state('');

  constructor(
    private readonly read = tauriGetLaunchAtLogin,
    private readonly write = tauriSetLaunchAtLogin,
  ) {}

  get enabled(): boolean {
    return this.state?.status === 'enabled';
  }

  get unavailableReason(): string | null {
    return this.state?.status === 'unavailable' ? this.state.reason : null;
  }

  async load(): Promise<void> {
    try {
      this.state = await this.read();
    } catch (cause) {
      this.state = { status: 'unavailable', reason: String(cause) };
    }
  }

  async set(enabled: boolean): Promise<void> {
    if (this.busy || this.state?.status === 'unavailable') return;
    this.busy = true;
    this.error = '';
    try {
      this.state = await this.write(enabled);
    } catch (cause) {
      this.error = String(cause);
    } finally {
      this.busy = false;
    }
  }
}

export const launchAtLoginStore = new LaunchAtLoginStore();
