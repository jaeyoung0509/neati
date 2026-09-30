<script lang="ts">
  import { scanStore } from '../stores/scan.svelte';
  import Button from './Button.svelte';
  import { scanInspectionDetails } from '../utils/scanInspectionDetails';
  import StorageAccessSetup from './StorageAccessSetup.svelte';

  let { compact = false }: { compact?: boolean } = $props();

  let fullDiskAccessGapCount = $derived(
    scanStore.lastScan?.gaps
      ?.filter((gap) => gap.kind === 'full_disk_access')
      .reduce((total, gap) => total + gap.count, 0) ?? 0
  );
  let inspectionDetails = $derived(scanStore.lastScan ? scanInspectionDetails(scanStore.lastScan) : []);
  let hasFullDiskAccessGap = $derived(fullDiskAccessGapCount > 0);

</script>

{#if scanStore.freshness !== 'fresh' || scanStore.discovery.status !== 'exhausted'}
  <div class="flex flex-wrap items-center justify-between gap-3 text-meta {compact ? 'border-b border-border py-2' : 'rounded-xl border border-border bg-secondary p-3'}" role="status">
    <div class="min-w-0 flex-1">
      {#if scanStore.freshness === 'refreshing'}
        Checking storage…
      {:else if scanStore.discovery.status === 'paused'}
        Checking storage…
      {:else if scanStore.discovery.status === 'stopped'}
        {scanStore.discovery.reason}
      {:else if scanStore.freshness === 'partial' && scanStore.cancelledScanNotice}
        {scanStore.cancelledScanNotice}
      {:else if scanStore.freshness === 'partial'}
        {#if hasFullDiskAccessGap}
          Some protected locations could not be checked. Unknown bytes are excluded from totals. Only verified items can be cleaned.
        {:else}
          Some locations could not be checked. Unknown bytes are excluded from totals. Only verified items can be cleaned.
        {/if}
      {:else if scanStore.freshness === 'unavailable'}
        {#if hasFullDiskAccessGap}
          Protected locations could not be checked. Unknown bytes are excluded from totals. Review storage access below.
        {:else}
          Storage could not be checked. Unknown bytes are excluded from totals. Try scanning again.
        {/if}
      {:else if scanStore.lastScan}
        Results are out of date. Scan again.
      {:else}
        Scan storage to find current cleanup candidates.
      {/if}
      {#if (scanStore.freshness === 'partial' || scanStore.freshness === 'unavailable') && !scanStore.isScanning && inspectionDetails.length > 0}
        <ul class="mt-2 space-y-1">
          {#each inspectionDetails as detail (detail.kind)}
            <li>{detail.label} · {detail.count}</li>
          {/each}
        </ul>
      {/if}
    </div>
    <span class="flex shrink-0 flex-wrap items-center gap-2">
      {#if !scanStore.canContinue && !hasFullDiskAccessGap}
        <Button size="sm" variant="outline" disabled={scanStore.isScanning || scanStore.isCleaning} onclick={() => scanStore.runScan()}>
          {scanStore.isScanning ? 'Scanning…' : 'Scan Again'}
        </Button>
      {/if}
    </span>
  </div>
{/if}

{#if hasFullDiskAccessGap}
  <div class="rounded-xl border border-border bg-secondary p-3">
    <StorageAccessSetup contextual />
  </div>
{/if}
