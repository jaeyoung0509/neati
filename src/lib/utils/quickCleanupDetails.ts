import type { ScanResult } from '../models/types';
import { isActionable, isAutoCleanable } from './cleanup';

import { scanInspectionDetails } from './scanInspectionDetails';

/** Presentation of backend facts only; this does not authorize cleanup. */
export function quickCleanupDetails(scan: ScanResult, quickEligibleCount: number) {
  const items = scan.categories.flatMap(category => category.items);
  const reviewCount = items.filter(item => isActionable(item) && !isAutoCleanable(item)).length;
  const excludedAutomaticCount = Math.max(0, items.filter(isAutoCleanable).length - quickEligibleCount);
  const count = (eligibility: string) => items.filter(item => item.disposition?.eligibility === eligibility).length;
  return {
    reviewCount,
    excludedAutomaticCount,
    blockedCount: count('blocked'),
    recentCount: count('recent'),
    advisoryCount: count('advisory'),
    policyGatedCount: count('policy_gated'),
    gaps: scanInspectionDetails(scan),
  };
}

/** The primary reason a current inventory cannot offer direct cleanup. */
export function quickCleanupUnavailableReason(scan: ScanResult): string {
  const details = quickCleanupDetails(scan, 0);
  if (details.reviewCount > 0) return `${details.reviewCount} ${details.reviewCount === 1 ? 'item needs' : 'items need'} review in Storage`;
  if (details.excludedAutomaticCount > 0) return 'Caches excluded by cleanup settings';
  if (details.recentCount > 0) return 'Remaining caches are too recent';
  if (details.blockedCount > 0) return 'Remaining items are protected or inaccessible';
  if (details.policyGatedCount > 0) return 'Items excluded by cleanup policy';
  if (details.advisoryCount > 0) return 'Remaining items need tool-managed cleanup';
  return scan.quality === 'partial' ? 'No eligible caches in checked locations' : 'No caches to clean';
}
