import type { RemovalResult } from '../models/types';

/**
 * How a reviewed removal ended, worded once for every reviewed workflow
 * (#528). When nothing was removed the reasons are the whole answer, so they
 * are shown instead of being folded away.
 */
export interface RemovalOutcome {
  headline: string;
  keptCount: number;
  nothingRemoved: boolean;
  /** Distinct kept reasons, each with how many items it kept. */
  reasons: { message: string; count: number }[];
}

export function removalOutcome(result: RemovalResult, singular: string, plural: string): RemovalOutcome {
  const keptCount = result.failed_count + result.skipped_count;
  const nothingRemoved = result.removed_count === 0 && keptCount > 0;
  const counts = new Map<string, number>();
  for (const item of result.items) {
    if (item.success) continue;
    const message = item.message.trim() || 'No reason was reported.';
    counts.set(message, (counts.get(message) ?? 0) + 1);
  }
  const noun = result.removed_count === 1 ? singular : plural;
  const headline = nothingRemoved
    ? `Nothing was removed · ${keptCount} kept`
    : `Permanently removed ${result.removed_count} ${noun}${keptCount > 0 ? ` · ${keptCount} kept` : ''}`;
  return {
    headline,
    keptCount,
    nothingRemoved,
    reasons: [...counts].map(([message, count]) => ({ message, count })),
  };
}
