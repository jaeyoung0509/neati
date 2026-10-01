import type { TemporaryReviewPreview, TemporaryStorageInventory, TrashResult } from '../models/types';
import { tauriStartTemporaryStorageScan, tauriCancelTemporaryStorageScan, tauriPrepareTemporaryStorageReview, tauriExecuteTemporaryStorageReview, tauriCancelTemporaryStorageReview } from '../utils/tauri';

export class TemporaryReviewStore {
  inventory = $state<TemporaryStorageInventory | null>(null);
  selectedIds = $state<string[]>([]);
  preview = $state<TemporaryReviewPreview | null>(null);
  result = $state<TrashResult | null>(null);
  busy = $state<'scan' | 'prepare' | 'execute' | null>(null);
  error = $state('');
  activeScanId: string | null = null;
  private disposed = false;
  private stopRequested = false;
  constructor(seed: TemporaryStorageInventory | null = null) { this.inventory = seed; }
  choose(itemId: string, optionId: string, checked: boolean) {
    const item = this.inventory?.items.find(item => item.id === itemId);
    if (!item) return;
    const ownIds = new Set(item.options.map(option => option.id));
    this.selectedIds = this.selectedIds.filter(id => !ownIds.has(id));
    if (checked) this.selectedIds = [...this.selectedIds, optionId];
    this.preview = null;
  }
  async scan() {
    this.stopRequested = false;
    this.busy = 'scan'; this.error = ''; this.preview = null; this.selectedIds = []; this.result = null;
    try {
      const result = await tauriStartTemporaryStorageScan(event => {
        if (event.type === 'started') {
          this.activeScanId = event.scan_id;
          if (this.disposed || this.stopRequested) void tauriCancelTemporaryStorageScan(event.scan_id).catch(() => {});
        }
      });
      if (!this.disposed) this.inventory = result;
    } catch (cause) { if (!this.disposed) this.error = String(cause); }
    finally { this.busy = null; this.activeScanId = null; }
  }
  async prepare() {
    if (!this.inventory || this.busy) return;
    this.busy = 'prepare'; this.error = '';
    try {
      const preview = await tauriPrepareTemporaryStorageReview(this.inventory.scan_id, this.selectedIds);
      if (!this.disposed) this.preview = preview;
      else void tauriCancelTemporaryStorageReview(preview.id).catch(() => {});
    }
    catch (cause) { if (!this.disposed) this.error = String(cause); }
    finally { this.busy = null; }
  }
  async execute(acceptUnknown: boolean, acceptSource: boolean) {
    if (!this.preview || this.busy) return;
    this.busy = 'execute'; this.error = '';
    try {
      const result = await tauriExecuteTemporaryStorageReview(this.preview.id, { confirmed: true, accept_unknown_usage: acceptUnknown, accept_source_loss: acceptSource });
      if (!this.disposed) { this.result = result; this.selectedIds = []; }
    } catch (cause) { if (!this.disposed) this.error = String(cause); }
    finally { this.busy = null; }
  }
  async stop() {
    try {
      if (this.busy === 'scan') this.stopRequested = true;
      if (this.busy === 'scan' && this.activeScanId) await tauriCancelTemporaryStorageScan(this.activeScanId);
      if (this.busy === 'execute' && this.preview) await tauriCancelTemporaryStorageReview(this.preview.id);
    } catch (cause) { if (!this.disposed && this.busy) this.error = String(cause); }
  }
  dismiss() {
    const id = this.preview?.id;
    this.preview = null;
    if (id && !this.result) void tauriCancelTemporaryStorageReview(id).catch(() => {});
  }
  dispose() { this.disposed = true; void this.stop(); if (this.busy !== 'execute') this.dismiss(); }
}
