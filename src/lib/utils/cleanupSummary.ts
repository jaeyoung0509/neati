import type { ScanDiscovery } from '../models/types';

export type CleanupSummaryState =
  | 'unavailable'
  | 'unknown'
  | 'scanning'
  | 'refreshing'
  | 'cleaning'
  | 'failed'
  | 'paused'
  | 'stopped'
  | 'stale'
  | 'partial'
  | 'ready'
  | 'clean';

interface CleanupSummaryFacts {
  available: boolean;
  hasScan: boolean;
  scanning: boolean;
  cleaning: boolean;
  freshness: 'empty' | 'fresh' | 'partial' | 'unavailable' | 'stale' | 'refreshing' | 'failed';
  discovery: ScanDiscovery;
  cleanableBytes: number;
}

/** Shared presentation phase for the Overview and Quick Panel summaries. */
export function cleanupSummaryState(facts: CleanupSummaryFacts): CleanupSummaryState {
  if (!facts.available) return 'unavailable';
  if (facts.cleaning) return 'cleaning';
  if (facts.scanning) return facts.hasScan ? 'refreshing' : 'scanning';
  if (facts.freshness === 'failed') return 'failed';
  if (!facts.hasScan) return 'unknown';
  if (facts.freshness === 'stale' || facts.freshness === 'unavailable') return 'stale';
  if (facts.discovery.status !== 'exhausted') return facts.discovery.status;
  if (facts.freshness === 'partial') return 'partial';
  return facts.cleanableBytes > 0 ? 'ready' : 'clean';
}

/** Coverage can be partial after completion; these states lack scan authority. */
export function scanCompletionNotice(discovery: ScanDiscovery): string | null {
  switch (discovery.status) {
    case 'paused': return 'Scan incomplete. Finish the scan before cleaning.';
    case 'stopped': return 'Scan stopped. Scan again before cleaning.';
    case 'exhausted': return null;
  }
}
