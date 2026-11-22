import type {
  DeveloperArtifact,
  DeveloperArtifactScanEvent,
  DeveloperArtifactScanResult,
  RemovalResult,
} from '../models/types';
import {
  tauriExecuteRemovalPlan,
  tauriPrepareDeveloperArtifactCleanup,
  tauriRestoreDeveloperWorkspaces,
  tauriStartDeveloperArtifactScan,
} from '../utils/tauri';
import { nonOverlappingArtifactIds } from '../utils/developerArtifactSelection';
import { developerArtifactKindLabel } from '../utils/developerArtifactKinds';
import { settingsStore } from './settings.svelte';

export type ProjectCleanupPhase = 'idle' | 'scanning' | 'ready' | 'cleaning' | 'error';

/**
 * Project build output for one-button Clean (#494).
 *
 * The backend decides what is preselected (idle, verified compiler build
 * output in saved project folders); this store only scans the saved folders,
 * keeps the person's choice, and removes the chosen units through the same
 * one-shot developer-artifact plan the Projects view uses.
 */
export class ProjectCleanupStore {
  phase = $state<ProjectCleanupPhase>('idle');
  result = $state<DeveloperArtifactScanResult | null>(null);
  selectedIds = $state<string[]>([]);
  activity = $state('');
  error = $state<string | null>(null);
  lastRemoval = $state<RemovalResult | null>(null);
  private generation = 0;

  /** Build outputs the backend offered for one-button Clean. */
  get items(): DeveloperArtifact[] {
    return (this.result?.items ?? []).filter((item) => item.selected_by_default);
  }

  get selectedItems(): DeveloperArtifact[] {
    const selected = new Set(this.selectedIds);
    return this.items.filter((item) => selected.has(item.id));
  }

  get selectedBytes(): number {
    return this.selectedItems.reduce((total, item) => total + item.allocated_bytes, 0);
  }

  get hasSavedFolders(): boolean {
    return (settingsStore.settings.project_folders ?? []).length > 0;
  }

  /** Scans the saved project folders. Nothing happens without one. */
  async refresh(): Promise<void> {
    const generation = ++this.generation;
    this.error = null;
    if (!this.hasSavedFolders) {
      this.reset();
      return;
    }
    this.phase = 'scanning';
    this.activity = '';
    try {
      const restore = await tauriRestoreDeveloperWorkspaces();
      if (generation !== this.generation) return;
      const workspaceIds = restore.workspaces.map((workspace) => workspace.id);
      if (workspaceIds.length === 0) {
        this.reset();
        return;
      }
      const result = await tauriStartDeveloperArtifactScan(workspaceIds, (event) =>
        this.observe(generation, event)
      );
      if (generation !== this.generation) return;
      this.result = result;
      this.selectedIds = nonOverlappingArtifactIds(this.items);
      this.phase = 'ready';
    } catch (cause) {
      if (generation !== this.generation) return;
      this.error = cause instanceof Error ? cause.message : String(cause);
      this.phase = 'error';
    } finally {
      if (generation === this.generation) this.activity = '';
    }
  }

  toggle(id: string, selected: boolean) {
    const others = this.selectedIds.filter((candidate) => candidate !== id);
    this.selectedIds = selected ? [...others, id] : others;
  }

  /**
   * Permanently removes the selected build outputs. The backend re-derives
   * ownership and repeats the open-file check right before each removal.
   */
  async clean(): Promise<RemovalResult | null> {
    if (!this.result || this.selectedIds.length === 0 || this.phase === 'cleaning') return null;
    const generation = ++this.generation;
    this.phase = 'cleaning';
    this.error = null;
    try {
      const plan = await tauriPrepareDeveloperArtifactCleanup(this.result.scan_id, this.selectedIds);
      const removal = await tauriExecuteRemovalPlan(plan.id);
      if (generation !== this.generation) return removal;
      this.lastRemoval = removal;
      const removed = new Set(removal.items.filter((item) => item.success).map((item) => item.item_id));
      this.result = {
        ...this.result,
        items: this.result.items.filter((item) => !removed.has(item.id)),
      };
      this.selectedIds = this.selectedIds.filter((id) => !removed.has(id));
      this.phase = 'ready';
      return removal;
    } catch (cause) {
      if (generation === this.generation) {
        this.error = cause instanceof Error ? cause.message : String(cause);
        this.phase = 'error';
      }
      return null;
    }
  }

  private observe(generation: number, event: DeveloperArtifactScanEvent) {
    if (generation !== this.generation) return;
    if (event.type === 'project_discovered') this.activity = `Found ${event.project_name}`;
    if (event.type === 'artifact_measurement_started') {
      this.activity = `Measuring ${event.project_name} · ${developerArtifactKindLabel(event.kind)}`;
    }
  }

  private reset() {
    this.phase = 'idle';
    this.result = null;
    this.selectedIds = [];
    this.activity = '';
  }
}

export const projectCleanupStore = new ProjectCleanupStore();
