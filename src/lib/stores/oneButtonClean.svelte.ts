import type { OneButtonOutcome, OneButtonPreview } from '../models/types';
import {
  refusalForPreview,
  tauriExecuteOneButtonClean,
  tauriPreviewOneButtonClean,
} from '../utils/tauri';
import { describeOneButtonActivity } from '../utils/oneButtonClean';

export type OneButtonPhase = 'idle' | 'checking' | 'ready' | 'cleaning' | 'error';

/**
 * One-button Clean through the backend use case (#500).
 *
 * The backend chooses the set (verified caches of the current scan plus idle,
 * verified build output), holds the preview, and executes exactly that preview
 * by id. This store only asks for a preview, shows it, and submits its id.
 */
export class OneButtonCleanStore {
  phase = $state<OneButtonPhase>('idle');
  preview = $state<OneButtonPreview | null>(null);
  outcome = $state<OneButtonOutcome | null>(null);
  activity = $state('');
  error = $state<string | null>(null);
  /** When the current check or removal started, for elapsed time (#491). */
  startedAt = $state<number | null>(null);
  private generation = 0;

  /** The browser preview must never pretend to delete; tests may state otherwise. */
  constructor(private readonly refusal: (action: string) => string | null = refusalForPreview) {}

  get totalBytes(): number {
    return this.preview?.total_bytes ?? 0;
  }

  get itemCount(): number {
    return this.preview?.items.length ?? 0;
  }

  /** Asks the backend for the current one-button set. */
  async refresh(): Promise<void> {
    if (this.phase === 'cleaning') return;
    const generation = ++this.generation;
    this.phase = 'checking';
    this.error = null;
    this.activity = '';
    this.startedAt = Date.now();
    try {
      const preview = await tauriPreviewOneButtonClean((event) => {
        if (generation === this.generation) this.activity = describeOneButtonActivity(event);
      });
      if (generation !== this.generation) return;
      this.preview = preview;
      this.phase = 'ready';
    } catch (cause) {
      if (generation !== this.generation) return;
      this.preview = null;
      this.error = cause instanceof Error ? cause.message : String(cause);
      this.phase = 'error';
    } finally {
      if (generation === this.generation) this.activity = '';
    }
  }

  /** Permanently removes exactly the shown preview, once. */
  async clean(): Promise<OneButtonOutcome | null> {
    const preview = this.preview;
    if (!preview || preview.items.length === 0 || this.phase !== 'ready') return null;
    const refused = this.refusal('Cleaning');
    if (refused) {
      this.error = refused;
      return null;
    }
    const generation = ++this.generation;
    this.phase = 'cleaning';
    this.error = null;
    this.startedAt = Date.now();
    // The id is one-shot: drop it now so nothing can submit it twice.
    this.preview = null;
    try {
      const outcome = await tauriExecuteOneButtonClean(preview.id, (event) => {
        if (generation === this.generation) this.activity = describeOneButtonActivity(event);
      });
      if (generation === this.generation) {
        this.outcome = outcome;
        this.phase = 'idle';
      }
      return outcome;
    } catch (cause) {
      if (generation === this.generation) {
        this.error = cause instanceof Error ? cause.message : String(cause);
        this.phase = 'error';
      }
      return null;
    } finally {
      if (generation === this.generation) this.activity = '';
    }
  }

  /** Forgets the preview, e.g. when the panel hides. */
  reset() {
    if (this.phase === 'cleaning') return;
    this.generation += 1;
    this.preview = null;
    this.phase = 'idle';
    this.activity = '';
  }
}

export const oneButtonCleanStore = new OneButtonCleanStore();
