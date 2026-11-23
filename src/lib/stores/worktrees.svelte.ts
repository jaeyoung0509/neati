import type {
  WorktreeInventory,
  WorktreeItem,
  WorktreeRemovalPreview,
  WorktreeRemovalResult,
} from '../models/types';
import { storageApi, type StorageManagementApi } from '../api/storage';

export type WorktreePhase = 'idle' | 'scanning' | 'ready' | 'confirming' | 'removing' | 'error';

type WorktreeApi = Pick<
  StorageManagementApi,
  'scanWorktrees' | 'prepareWorktreeRemoval' | 'executeWorktreeRemoval'
>;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * Finished Git worktrees (#505). The backend decides each worktree's state and
 * re-derives it before removal; this store only scans, keeps the person's
 * selection of finished ones, and submits the one-shot plan.
 */
export class WorktreeStore {
  phase = $state<WorktreePhase>('idle');
  inventory = $state<WorktreeInventory | null>(null);
  selectedIds = $state<string[]>([]);
  preview = $state<WorktreeRemovalPreview | null>(null);
  result = $state<WorktreeRemovalResult | null>(null);
  error = $state<string | null>(null);
  startedAt = $state<number | null>(null);
  private generation = 0;

  constructor(private readonly api: WorktreeApi = storageApi) {}

  get items(): WorktreeItem[] {
    return this.inventory?.items ?? [];
  }

  get finished(): WorktreeItem[] {
    return this.items.filter((item) => item.state === 'finished');
  }

  get selectedItems(): WorktreeItem[] {
    const selected = new Set(this.selectedIds);
    return this.finished.filter((item) => selected.has(item.id));
  }

  get selectedBytes(): number {
    return this.selectedItems.reduce((total, item) => total + item.allocated_bytes, 0);
  }

  async scan(): Promise<void> {
    if (this.phase === 'removing') return;
    const generation = ++this.generation;
    this.phase = 'scanning';
    this.error = null;
    this.preview = null;
    this.startedAt = Date.now();
    try {
      const inventory = await this.api.scanWorktrees();
      if (generation !== this.generation) return;
      this.inventory = inventory;
      const finished = new Set(this.finished.map((item) => item.id));
      this.selectedIds = this.selectedIds.filter((id) => finished.has(id));
      this.phase = 'ready';
    } catch (cause) {
      if (generation !== this.generation) return;
      this.error = message(cause);
      this.phase = 'error';
    }
  }

  toggle(id: string) {
    if (!this.finished.some((item) => item.id === id)) return;
    this.selectedIds = this.selectedIds.includes(id)
      ? this.selectedIds.filter((selected) => selected !== id)
      : [...this.selectedIds, id];
  }

  /** Asks the backend to hold the selection as a plan, then asks for confirmation. */
  async review(): Promise<void> {
    if (!this.inventory || this.selectedItems.length === 0 || this.phase !== 'ready') return;
    this.error = null;
    try {
      this.preview = await this.api.prepareWorktreeRemoval(
        this.inventory.scan_id,
        this.selectedItems.map((item) => item.id)
      );
      this.phase = 'confirming';
    } catch (cause) {
      this.error = message(cause);
    }
  }

  cancelReview() {
    if (this.phase !== 'confirming') return;
    this.preview = null;
    this.phase = 'ready';
  }

  /** Removes exactly the confirmed plan, once, then checks again. */
  async remove(): Promise<WorktreeRemovalResult | null> {
    const preview = this.preview;
    if (!preview || this.phase !== 'confirming') return null;
    this.phase = 'removing';
    this.error = null;
    this.startedAt = Date.now();
    this.preview = null;
    try {
      const result = await this.api.executeWorktreeRemoval(preview.plan_id);
      this.result = result;
      this.selectedIds = [];
      this.phase = 'ready';
      await this.scan();
      return result;
    } catch (cause) {
      this.error = message(cause);
      this.phase = 'ready';
      return null;
    }
  }
}

export const worktreeStore = new WorktreeStore();
