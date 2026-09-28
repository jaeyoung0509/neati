<script lang="ts">
  import { scanStore } from '../stores/scan.svelte';
  import { cleanupAvailability, observedByteRange, summarizeCategory } from '../utils/cleanup';
  import { formatBytes } from '../utils/format';

  let scan = $derived(scanStore.lastScan);
  let summary = $derived(summarizeCategory(scan?.categories.flatMap(category => category.items) ?? []));
  let availability = $derived(cleanupAvailability(scan?.categories.flatMap(category => category.items) ?? []));
  let observed = $derived(observedByteRange(scan?.total_bytes ?? 0, scan?.ambiguous_overlap_bytes));
  let isCurrent = $derived(scanStore.freshness === 'fresh' || scanStore.freshness === 'partial');
  let estimateLabel = $derived(isCurrent ? 'Cleanup candidates' : 'Last cleanup estimate');
</script>

<section class="storage-summary" aria-label="Storage scan summary">
  <div class="summary-primary">
    <p class="text-meta font-medium text-muted-foreground">{scan ? estimateLabel : 'Available to clean'}</p>
    <p class="mt-1 text-metric-lg font-mono font-semibold tracking-tight tabular-nums text-foreground">
      {scan ? summary.cleanable_bytes > 0 ? formatBytes(summary.cleanable_bytes) : summary.cleanable_count > 0 ? 'Not estimated' : formatBytes(0) : '—'}
    </p>
    <p class="mt-1 text-meta text-muted-foreground">
      {#if !scan}
        Scan caches to see what can be cleaned.
      {:else if !isCurrent}
        Scan again to verify these results.
      {:else if summary.cleanable_count > 0}
        Apps may download or rebuild these caches later.
      {:else}
        No cleanup candidates in this scan.
      {/if}
    </p>
    {#if isCurrent && summary.unestimated_count > 0}
      <p class="mt-1 text-meta text-muted-foreground">{summary.unestimated_count} tool-managed {summary.unestimated_count === 1 ? 'cleanup has' : 'cleanups have'} no size estimate. The tool decides what is unused.</p>
    {/if}
    {#if isCurrent}
      <p class="mt-2 text-meta text-muted-foreground">
        <span class="font-medium text-foreground">{formatBytes(availability.ready)} ready now</span>
        {#if availability.running > 0} · {formatBytes(availability.running)} after closing apps{/if}
        {#if availability.review > 0} · {formatBytes(availability.review)} needs review{/if}
      </p>
    {/if}
  </div>
  <div class="summary-context">
    <p class="text-meta text-muted-foreground">Found in scanned locations</p>
    <p class="mt-1 text-sm font-medium font-mono tabular-nums text-foreground">
      {#if !scan}—
      {:else if observed.isAmbiguous}{formatBytes(observed.lower)}–{formatBytes(observed.upper)}
      {:else}{formatBytes(observed.upper)}{/if}
    </p>
    <p class="mt-1 text-meta text-muted-foreground">Includes items that must be kept.</p>
  </div>
</section>

<style>
  .storage-summary {
    display: grid;
    grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr);
    align-items: center;
    gap: 24px;
    padding: 0;
  }
  .summary-primary { padding-left: 16px; border-left: 3px solid hsl(var(--primary)); }
  .summary-context { border-left: 1px solid hsl(var(--border)); padding-left: 24px; }
  @container (max-width: 460px) {
    .storage-summary { grid-template-columns: minmax(0, 1fr); gap: 16px; }
    .summary-context { border: 0; padding-left: 19px; }
  }
</style>
