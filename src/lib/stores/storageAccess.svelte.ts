import { isTauri } from '../api';
import { tauriOpenFullDiskAccessSettings } from '../utils/tauri';
import { scanStore } from './scan.svelte';

type AccessPhase = 'idle' | 'opening' | 'waiting' | 'queued' | 'checking' | 'checked' | 'failed';

interface AccessRecoveryPorts {
  openSettings: () => Promise<void>;
  scan: () => Promise<boolean>;
  busy: () => boolean;
  native: () => boolean;
}

/** A Settings round trip requests a fresh scan; it never establishes a grant. */
export class StorageAccessStore {
  phase = $state<AccessPhase>('idle');
  error = $state<string | null>(null);
  private subscribers = 0;
  private disposeEvents: (() => void) | null = null;
  private generation = 0;
  private leftApp = false;
  private returnedToApp = false;

  constructor(private readonly ports: AccessRecoveryPorts) {}

  subscribe(): () => void {
    this.subscribers++;
    if (this.subscribers === 1 && typeof window !== 'undefined') {
      const leave = () => {
        if (this.phase === 'opening' || this.phase === 'waiting') this.leftApp = true;
      };
      const activate = () => {
        if (document.visibilityState === 'hidden') return;
        if (this.phase === 'queued') {
          void this.check();
          return;
        }
        if (this.leftApp) {
          this.returnedToApp = true;
          if (this.phase === 'waiting') {
            this.phase = 'queued';
            void this.check();
          }
        }
      };
      const visibility = () => {
        if (document.visibilityState === 'hidden') leave();
        else activate();
      };
      window.addEventListener('blur', leave);
      window.addEventListener('focus', activate);
      document.addEventListener('visibilitychange', visibility);
      this.disposeEvents = () => {
        window.removeEventListener('blur', leave);
        window.removeEventListener('focus', activate);
        document.removeEventListener('visibilitychange', visibility);
      };
    }
    let disposed = false;
    return () => {
      if (disposed) return;
      disposed = true;
      if (--this.subscribers === 0) {
        this.disposeEvents?.();
        this.disposeEvents = null;
        this.generation++;
        this.phase = 'idle';
        this.leftApp = false;
        this.error = null;
      }
    };
  }

  async openSettings(): Promise<void> {
    if (this.phase === 'opening' || this.phase === 'checking') return;
    this.error = null;
    if (!this.ports.native()) {
      this.phase = 'failed';
      this.error = 'Open the desktop app to change macOS access settings. Browser preview cannot open System Settings.';
      return;
    }
    const generation = ++this.generation;
    this.phase = 'opening';
    this.leftApp = false;
    this.returnedToApp = false;
    try {
      await this.ports.openSettings();
      if (generation !== this.generation) return;
      this.phase = 'waiting';
      if (this.returnedToApp) {
        this.phase = 'queued';
        void this.check();
      }
    } catch (error) {
      if (generation !== this.generation) return;
      this.phase = 'failed';
      this.error = error instanceof Error ? error.message : String(error);
    }
  }

  /** Deferred return-to-app work runs only when the owning scan store is idle. */
  checkWhenIdle(): void {
    if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
    if (this.phase === 'queued' && !this.ports.busy()) void this.check();
  }

  async check(): Promise<void> {
    if (this.phase === 'opening' || this.phase === 'checking') return;
    if (!this.ports.native()) {
      this.phase = 'failed';
      this.error = 'Access checks require the desktop app. Browser preview does not inspect macOS permissions.';
      return;
    }
    if (this.ports.busy()) {
      this.phase = 'queued';
      return;
    }
    const generation = ++this.generation;
    this.phase = 'checking';
    this.error = null;
    this.leftApp = false;
    try {
      const checked = await this.ports.scan();
      if (generation !== this.generation) return;
      this.phase = checked ? 'checked' : 'failed';
      if (!checked) this.error = 'Storage could not be rechecked. Try scanning again.';
    } catch (error) {
      if (generation !== this.generation) return;
      this.phase = 'failed';
      this.error = error instanceof Error ? error.message : String(error);
    }
  }
}

export const storageAccessStore = new StorageAccessStore({
  openSettings: tauriOpenFullDiskAccessSettings,
  scan: async () => (await scanStore.runScan()) !== null,
  busy: () => scanStore.isScanning || scanStore.isCleaning,
  native: isTauri,
});
