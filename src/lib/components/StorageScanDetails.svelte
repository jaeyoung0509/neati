<script lang="ts">
  import type { ScanDiscovery, ScanResult } from '../models/types';
  import { retainedByteGroups, summarizeCategory } from '../utils/cleanup';
  import { scanInspectionDetails } from '../utils/scanInspectionDetails';
  import { formatBytes } from '../utils/format';

  let { scan, discovery }: { scan: ScanResult; discovery: ScanDiscovery } = $props();
  let items = $derived(scan.categories.flatMap(category => category.items));
  let retained = $derived(retainedByteGroups(items));
  let summary = $derived(summarizeCategory(items));
  let inspectionDetails = $derived(scanInspectionDetails(scan));
</script>

<details class="text-meta text-muted-foreground" data-storage-scan-details>
  <summary class="w-fit cursor-pointer rounded-sm py-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">Scan details</summary>
  <div class="mt-2 space-y-3 rounded-xl border border-border bg-card p-3">
    {#if discovery.status === 'stopped'}<p>{discovery.reason}</p>{/if}
    <p>Only verified items from a completed, current scan can be cleaned. Unread locations remain unknown and are excluded from totals. Apps may download or rebuild cleaned caches later.</p>
    {#if (scan.ambiguous_overlap_bytes ?? 0) > 0}
      <p>Some observations overlap. The displayed range accounts for uncertain shared bytes; row and reason totals are upper bounds, not unique disk usage.</p>
    {/if}
    {#if summary.unestimated_count > 0}
      <p>{summary.unestimated_count} tool-managed {summary.unestimated_count === 1 ? 'cleanup has' : 'cleanups have'} no size estimate. The tool decides what is unused; Not estimated does not mean zero bytes.</p>
    {/if}
    {#if retained.length > 0}
      <div>
        <h3 class="font-medium text-foreground">Why some bytes stay</h3>
        <p class="mt-1">Includes items that must be kept.</p>
        <dl class="mt-2 space-y-1" aria-label="Retained bytes by reason">
          {#each retained as group (group.kind)}
            <div class="flex flex-wrap justify-between gap-x-3"><dt>{group.label}</dt><dd class="font-mono tabular-nums text-foreground">{formatBytes(group.bytes)}</dd></div>
          {/each}
        </dl>
      </div>
    {/if}
    {#if inspectionDetails.length > 0}
      <div>
        <h3 class="font-medium text-foreground">Inspection reasons</h3>
        <ul class="mt-2 space-y-1">
          {#each inspectionDetails as detail (detail.kind)}
            <li>{detail.label} · {detail.count}</li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>
</details>
