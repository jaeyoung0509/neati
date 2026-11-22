import type { CleanupHistoryEntry } from '../models/types';

export interface CleanupHistoryTotals {
  runs: number;
  removedBytes: number;
}

export function cleanupHistoryTotals(entries: readonly CleanupHistoryEntry[]): CleanupHistoryTotals {
  return {
    runs: entries.length,
    removedBytes: entries.reduce((total, entry) => total + entry.removed_bytes, 0),
  };
}

/** The part of a run that did not fully succeed, or '' when everything did. */
export function cleanupHistoryIssues(entry: CleanupHistoryEntry): string {
  const parts: string[] = [];
  if (entry.partial_count > 0) parts.push(`${entry.partial_count} partly cleaned`);
  if (entry.failed_count > 0) parts.push(`${entry.failed_count} failed`);
  if (entry.skipped_count > 0) parts.push(`${entry.skipped_count} skipped`);
  return parts.join(' · ');
}

export function cleanupHistoryDate(entry: CleanupHistoryEntry, locale?: string): string {
  return new Date(entry.finished_at * 1000).toLocaleString(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}
