import type { ScanResult } from '../models/types';
import { cleanupAvailability } from './cleanup';

export type CleanupFreshness = 'empty' | 'fresh' | 'partial' | 'unavailable' | 'stale' | 'refreshing' | 'failed';

/** Presentation of measured facts only; this never makes an item actionable. */
export function cleanupView(scan: ScanResult | null, freshness: CleanupFreshness, failed: boolean) {
  const items = scan?.categories.flatMap(category => category.items) ?? [];
  const hasMeasuredResults = !!scan && (scan.quality === 'fresh'
    || scan.total_bytes > 0 || items.some(item => item.quality === 'fresh' || item.quality === 'partial'));
  const current = freshness === 'fresh' || freshness === 'partial';
  let title = 'Start with a storage scan';
  let description = 'Use Scan Storage to inspect application and development caches. Scanning does not delete anything.';
  let kind: 'initial' | 'failed' | 'partial' | 'stale' | 'empty' | 'retained' | 'available' = 'initial';
  if (failed || freshness === 'failed') {
    kind = 'failed'; title = 'The storage scan did not finish';
    description = 'Try Scan Storage again. Previous results, if shown, must be checked before cleaning.';
  } else if (freshness === 'unavailable' || freshness === 'partial') {
    kind = 'partial'; title = items.length > 0 ? 'Only checked items are shown' : 'No verified items to show yet';
    description = 'Unread locations remain unknown. Review the inspection reasons above, then scan again.';
  } else if (scan && !current) {
    kind = 'stale'; title = 'These results need a fresh scan';
    description = 'Use Scan Storage to check what is currently available. Previous results do not authorize cleanup.';
  } else if (scan && items.length === 0) {
    kind = 'empty'; title = 'No cache items found in checked locations';
    description = 'This scan found no matching cache items. Other files and locations are outside this result.';
  } else if (scan && cleanupAvailability(items).ready === 0) {
    kind = 'retained'; title = 'Nothing is ready to clean now';
    description = 'Items below are kept, require idle apps or need a supported owner action. Open a category to inspect the reasons.';
  } else if (scan) {
    kind = 'available'; title = 'Choose what to clean';
    description = 'Only verified cleanup items can be selected. Apps may rebuild these caches later.';
  }
  return { kind, title, description, items, current, hasMeasuredResults };
}
