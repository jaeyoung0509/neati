import type { PublishedScan, ScanDiscovery, ScanItem } from '../../lib/models/types';

/** Native-shaped retained inventory; synthetic paths and bytes, never filesystem IO. */
export function retainedScanFixture(
  discovery: ScanDiscovery,
  finishedAt = Math.floor(Date.now() / 1000),
  scanId = 'retained-scan-fixture',
): PublishedScan & { discovery: ScanDiscovery } {
  const items: ScanItem[] = [128, 128, 120].map((kib, index) => ({
    id: `retained-cache-${index}`,
    signature_id: `system.fixture.cache.${index}`,
    name: `Verified fixture cache ${index + 1}`,
    category: 'system',
    risk: 'safe',
    path: `/fixture/cache/${index}`,
    size: { logical: kib * 1024, allocated: kib * 1024 },
    file_count: 1,
    description: 'Synthetic retained unit',
    is_selected: true,
    last_modified: null,
    exists: true,
    quality: 'fresh',
    incomplete_reason: null,
    disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: kib * 1024 },
  }));
  const bytes = items.reduce((total, item) => total + item.size.allocated!, 0);
  const eligibility = { buckets: [
    { eligibility: 'auto_cleanable' as const, observed_bytes: bytes, cleanable_bytes: bytes, items: items.length },
    ...(['reviewable', 'recent', 'policy_gated', 'advisory', 'blocked'] as const)
      .map(eligibility => ({ eligibility, observed_bytes: 0, cleanable_bytes: 0, items: 0 })),
  ] };
  const cancelled = discovery.status === 'stopped';
  return {
    discovery,
    result: {
      scan_id: scanId,
      valid_for_seconds: 300,
      started_at: finishedAt - 1,
      finished_at: finishedAt,
      categories: [{
        category: 'system', display_name: 'System', items,
        total_bytes: bytes, cleanable_bytes: bytes, safe_bytes: bytes, rebuild_bytes: 0, manual_bytes: 0,
        quality: 'fresh', eligibility,
      }],
      total_bytes: bytes, cleanable_bytes: bytes, safe_bytes: bytes, rebuild_bytes: 0, manual_bytes: 0,
      // Coverage and completion are deliberately independent: every retained
      // unit is fully measured, even when the walk stopped or paused elsewhere.
      quality: 'partial',
      incomplete_reasons: cancelled ? ['Scan was cancelled before completion'] : [],
      gaps: cancelled ? [{ kind: 'cancelled', count: 1 }]
        : discovery.status === 'exhausted' ? [{ kind: 'permission_denied', count: 1 }] : [],
      cancelled,
      eligibility,
    },
  };
}
