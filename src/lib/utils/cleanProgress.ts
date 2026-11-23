import type { CleanEvent } from '../models/types';

/**
 * Phase and elapsed-time copy for long removals (#491), shared by Storage's
 * catalog cleanup and the Quick Panel. It only restates the progress events the
 * backend already delivers: no percentage, completion time or new telemetry is
 * invented here.
 */
export interface CleanProgress {
  /** Name of the target being removed, empty before the first one starts. */
  currentItem: string;
  /** 1-based index of the target being removed; 0 before the first one. */
  index: number;
  total: number;
  /** Share of targets already finished, 0–100. */
  percent: number;
}

export const IDLE_CLEAN_PROGRESS: CleanProgress = { currentItem: '', index: 0, total: 0, percent: 0 };

function share(done: number, total: number): number {
  return total > 0 ? Math.round((Math.min(done, total) / total) * 100) : 0;
}

/**
 * Folds one backend event into the progress shown. A target counts as done
 * when it finishes, not when it starts, so the bar never reads full while the
 * last (often largest) target is still being removed.
 */
export function reduceCleanProgress(progress: CleanProgress, event: CleanEvent): CleanProgress {
  switch (event.type) {
    case 'Started':
      return { currentItem: '', index: 0, total: event.total_targets, percent: 0 };
    case 'ItemStarted':
      return {
        currentItem: event.name,
        index: event.index,
        total: event.total,
        percent: share(event.index - 1, event.total),
      };
    case 'ItemFinished':
      return { ...progress, percent: share(progress.index, progress.total) };
    default:
      return progress;
  }
}

/** Whole seconds between a start time and now, never negative. */
export function elapsedSeconds(startedAt: number | null, now: number): number {
  return startedAt === null ? 0 : Math.max(0, Math.floor((now - startedAt) / 1000));
}

/** `8 s`, `2 min 5 s`, `1 h 4 min`. */
export function formatElapsed(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  if (whole < 60) return `${whole} s`;
  const minutes = Math.floor(whole / 60);
  if (minutes < 60) return `${minutes} min ${whole % 60} s`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

/**
 * The current removal phase: `Removing npm cache · 3 of 12`, or
 * `Starting cleanup · 12 items` before the first target starts.
 */
export function cleanPhaseLabel(name: string, index: number, total: number): string {
  if (index > 0 && name) return `Removing ${name} · ${index} of ${total}`;
  if (total > 0) return `Starting cleanup · ${total} item${total === 1 ? '' : 's'}`;
  return 'Starting cleanup';
}

/** `Removing npm cache · 3 of 12 · 8 s`. */
export function cleanProgressLabel(progress: CleanProgress, seconds: number): string {
  return `${cleanPhaseLabel(progress.currentItem, progress.index, progress.total)} · ${formatElapsed(seconds)}`;
}
