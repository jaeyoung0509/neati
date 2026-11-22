<script lang="ts">
  import type { ScanResult } from '../models/types';
  import { quickCleanupDetails } from '../utils/quickCleanupDetails';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { scan, quickEligibleCount, busy = false, onClose, onReview, onRescan }: {
    scan: ScanResult;
    quickEligibleCount: number;
    busy?: boolean;
    onClose: () => void;
    onReview: () => void;
    onRescan: () => void;
  } = $props();
  const id = $props.id();
  let details = $derived(quickCleanupDetails(scan, quickEligibleCount));

</script>

<Dialog
  onCancel={onClose}
  size="sm"
  density="compact"
  aria-labelledby={id + '-title'}
  aria-describedby={id + '-description'}
>
  <h2 id={id + '-title'} class="text-body font-semibold">Cleanup details</h2>
  <p id={id + '-description'} class="mt-2 text-meta leading-snug text-muted-foreground">
    {#if quickEligibleCount === 0}
      No items currently qualify for Quick Clean. Only verified Safe and Rebuild caches can be cleaned here.
    {:else}
      {quickEligibleCount} cache {quickEligibleCount === 1 ? 'item is' : 'items are'} available for direct cleanup.
    {/if}
  </p>
  {#if details.reviewCount > 0}
    <p class="mt-3 text-meta font-medium">
      {details.reviewCount} {details.reviewCount === 1 ? 'item needs' : 'items need'} review in Storage before cleanup.
    </p>
  {/if}
  {#if details.excludedAutomaticCount > 0}
    <p class="mt-2 text-meta text-muted-foreground">{details.excludedAutomaticCount} automatic {details.excludedAutomaticCount === 1 ? 'item is' : 'items are'} excluded by your cleanup category settings.</p>
  {/if}
  <ul class="mt-3 space-y-1 text-meta text-muted-foreground" aria-label="Items excluded from Quick Clean">
    {#if details.blockedCount > 0}<li>{details.blockedCount} blocked by safety or access checks</li>{/if}
    {#if details.recentCount > 0}<li>{details.recentCount} too recent to clean</li>{/if}
    {#if details.advisoryCount > 0}<li>{details.advisoryCount} managed outside automatic cleanup</li>{/if}
    {#if details.policyGatedCount > 0}<li>{details.policyGatedCount} excluded by cleanup policy</li>{/if}
  </ul>
  {#if scan.quality === 'partial'}
    <div class="mt-3 border-t border-border pt-3">
      <h3 class="text-meta font-medium">Why the scan is partial</h3>
      {#if details.gaps.length > 0}
        <ul class="mt-2 space-y-2 text-caption leading-snug text-muted-foreground">
          {#each details.gaps as gap}<li>{gap.label} ({gap.count}).</li>{/each}
        </ul>
      {:else}
        <p class="mt-2 text-meta text-muted-foreground">Some locations could not be fully checked. Open Storage for the scan details.</p>
      {/if}
      <p class="mt-2 text-caption text-muted-foreground">Unknown bytes are excluded. Scanning again will not resolve an unchanged access restriction.</p>
    </div>
  {/if}
  <div class="mt-4 flex flex-wrap justify-end gap-2">
    <Button size="sm" variant="ghost" onclick={onClose}>Close</Button>
    <Button size="sm" variant="secondary" disabled={busy} onclick={onRescan}>Scan Again</Button>
    <Button size="sm" variant="primary" onclick={onReview}>Open Storage</Button>
  </div>
</Dialog>
