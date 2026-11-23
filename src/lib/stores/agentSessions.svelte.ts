import { describeError } from '../utils/errors';
import { loadFailed, loadStarted, type LoadPhase } from '../utils/loadPhase';
import type {
  AgentSessionInventory,
  SessionBundlePreview,
  SessionExportOutcome,
  SessionExportPreview,
  SessionImportOutcome,
  SessionRemap,
} from '../models/types';
import {
  refusalForPreview,
  tauriCancelSessionOperation,
  tauriExportSessionBundle,
  tauriGetAgentSessions,
  tauriImportSessionBundle,
  tauriOpenSessionBundle,
  tauriPickSessionProjectFolder,
  tauriPrepareSessionExport,
} from '../utils/tauri';

export type ExportStep = 'idle' | 'preparing' | 'review' | 'writing';
export type ImportStep = 'idle' | 'opening' | 'review' | 'importing';

/**
 * The Sessions view's state (#508, #509). The inventory is read-only; an
 * export and an import each hold one backend preview id. The window never
 * submits a path: bundles and folders come from system dialogs the backend
 * opens.
 */
export class AgentSessionsStore {
  inventory = $state<AgentSessionInventory | null>(null);
  phase = $state<LoadPhase>('idle');
  /** A read is running, the first or a refresh. */
  reading = $state(false);
  error = $state<string | null>(null);
  selected = $state<string[]>([]);

  exportStep = $state<ExportStep>('idle');
  exportPreview = $state<SessionExportPreview | null>(null);
  exportError = $state<string | null>(null);
  exportResult = $state<SessionExportOutcome | null>(null);

  importStep = $state<ImportStep>('idle');
  bundle = $state<SessionBundlePreview | null>(null);
  remaps = $state<Record<string, string>>({});
  importError = $state<string | null>(null);
  importResult = $state<SessionImportOutcome | null>(null);

  private loading: Promise<void> | null = null;

  /** Session keys the current inventory still lists. */
  private known(): Set<string> {
    return new Set(this.inventory?.projects.flatMap((project) => project.sessions.map((entry) => entry.key)) ?? []);
  }

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
      const known = this.known();
      this.selected = this.selected.filter((key) => known.has(key));
    } catch (error) {
      this.error = describeError(error, 'Agent sessions could not be read.');
      this.phase = loadFailed(this.phase);
    } finally {
      this.reading = false;
    }
  }

  isSelected(key: string): boolean {
    return this.selected.includes(key);
  }

  setSelected(keys: string[], on: boolean): void {
    const next = new Set(this.selected);
    for (const key of keys) {
      if (on) next.add(key);
      else next.delete(key);
    }
    this.selected = [...next];
  }

  get selectedBytes(): number {
    const chosen = new Set(this.selected);
    return (
      this.inventory?.projects
        .flatMap((project) => project.sessions)
        .filter((entry) => chosen.has(entry.key))
        .reduce((sum, entry) => sum + entry.size_bytes, 0) ?? 0
    );
  }

  /** Counts likely secrets in the selection before anything is written. */
  async prepareExport(): Promise<void> {
    if (this.selected.length === 0 || this.exportStep !== 'idle') return;
    this.exportStep = 'preparing';
    this.exportError = null;
    this.exportResult = null;
    try {
      this.exportPreview = await tauriPrepareSessionExport([...this.selected]);
      this.exportStep = 'review';
    } catch (error) {
      this.exportError = describeError(error, 'The export could not be prepared.');
      this.exportStep = 'idle';
    }
  }

  closeExport(): void {
    if (this.exportStep === 'writing') return;
    this.exportStep = 'idle';
    this.exportPreview = null;
    this.exportError = null;
  }

  /** Asks where to save and writes the bundle. A closed dialog keeps the review open. */
  async exportBundle(allowSecrets: boolean): Promise<void> {
    const preview = this.exportPreview;
    if (!preview || this.exportStep !== 'review') return;
    const refusal = refusalForPreview('Writing a session bundle');
    if (refusal) {
      this.exportError = refusal;
      return;
    }
    this.exportStep = 'writing';
    this.exportError = null;
    try {
      const outcome = await tauriExportSessionBundle(preview.id, allowSecrets);
      if (!outcome) {
        this.exportStep = 'review';
        return;
      }
      this.exportResult = outcome;
      this.exportPreview = null;
      this.exportStep = 'idle';
    } catch (error) {
      this.exportError = describeError(error, 'The bundle could not be written.');
      this.exportStep = 'review';
    }
  }

  /** Asks for a bundle and verifies it. */
  async openBundle(): Promise<void> {
    if (this.importStep !== 'idle') return;
    this.importStep = 'opening';
    this.importError = null;
    this.importResult = null;
    try {
      const bundle = await tauriOpenSessionBundle();
      this.bundle = bundle;
      this.remaps = {};
      this.importStep = bundle ? 'review' : 'idle';
    } catch (error) {
      this.importError = describeError(error, 'The bundle could not be read.');
      this.importStep = 'idle';
    }
  }

  closeImport(): void {
    if (this.importStep === 'importing') return;
    this.importStep = 'idle';
    this.bundle = null;
    this.remaps = {};
    this.importError = null;
  }

  async chooseRemap(originalCwd: string): Promise<void> {
    try {
      const folder = await tauriPickSessionProjectFolder();
      if (folder) this.remaps = { ...this.remaps, [originalCwd]: folder };
    } catch (error) {
      this.importError = describeError(error, 'The folder dialog could not be opened.');
    }
  }

  clearRemap(originalCwd: string): void {
    const next = { ...this.remaps };
    delete next[originalCwd];
    this.remaps = next;
  }

  async importBundle(): Promise<void> {
    const bundle = this.bundle;
    if (!bundle || this.importStep !== 'review') return;
    const refusal = refusalForPreview('Importing sessions');
    if (refusal) {
      this.importError = refusal;
      return;
    }
    this.importStep = 'importing';
    this.importError = null;
    const remaps: SessionRemap[] = Object.entries(this.remaps).map(([from, to]) => ({ from, to }));
    try {
      this.importResult = await tauriImportSessionBundle(bundle.id, remaps);
      this.bundle = null;
      this.remaps = {};
      this.importStep = 'idle';
      await this.load();
    } catch (error) {
      this.importError = describeError(error, 'The sessions could not be imported.');
      this.importStep = 'review';
    }
  }

  /** Stops the running read, export or import. */
  async stop(): Promise<void> {
    await tauriCancelSessionOperation();
  }
}

export const agentSessionsStore = new AgentSessionsStore();
