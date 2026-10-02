import type { DockerStatus } from '../models/types';
import { refusalForPreview, tauriGetDockerStatus, tauriPruneDocker } from '../utils/tauri';

export class DockerStore {
  status = $state<DockerStatus | null>(null);
  isLoading = $state(false);
  isPruning = $state(false);
  error = $state<string | null>(null);

  private refreshPromise: Promise<void> | null = null;

  async refresh(afterMutation = false): Promise<void> {
    // Share only the same in-flight observation. A mutation must wait for an
    // older read and then obtain a fresh inventory; it never reuses that read.
    if (afterMutation && this.refreshPromise) await this.refreshPromise;
    if (this.refreshPromise) return this.refreshPromise;
    const pending = this.load();
    this.refreshPromise = pending;
    try {
      await pending;
    } finally {
      if (this.refreshPromise === pending) this.refreshPromise = null;
    }
  }

  private async load(): Promise<void> {
    this.isLoading = true;
    this.error = null;
    try {
      this.status = await tauriGetDockerStatus();
    } catch (e: any) {
      this.error = e?.toString() || 'Failed to fetch Docker status';
    } finally {
      this.isLoading = false;
    }
  }

  async pruneTarget(signatureId: string): Promise<number> {
    const refusal = refusalForPreview('Pruning Docker data');
    if (refusal) {
      this.error = refusal;
      return 0;
    }
    this.isPruning = true;
    this.error = null;
    try {
      const reclaimed = await tauriPruneDocker(signatureId);
      await this.refresh(true);
      return reclaimed;
    } catch (e: any) {
      this.error = e?.toString() || 'Failed to prune Docker target';
      return 0;
    } finally {
      this.isPruning = false;
    }
  }
}

export const dockerStore = new DockerStore();
