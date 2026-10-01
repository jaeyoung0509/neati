<script lang="ts">
  import Badge from './Badge.svelte';
  import Button from './Button.svelte';
  import AnimatedValue from './AnimatedValue.svelte';
  import RippleScene from './RippleScene.svelte';
  import { scanStore } from '../stores/scan.svelte';
  import { cleanupAvailability, observedByteRange, summarizeCategory } from '../utils/cleanup';
  import { formatBytes } from '../utils/format';
  import { cleanupView } from '../utils/cleanupView';

  interface Props {
    onQuit?: () => void;
    onReview?: () => void;
    actionsDisabled?: boolean;
  }
  let { onQuit, onReview, actionsDisabled = false }: Props = $props();
  let scan = $derived(scanStore.lastScan);
  let presentation = $derived(cleanupView(scan, scanStore.freshness, !!scanStore.error));
  let summary = $derived(summarizeCategory(presentation.items));
  let availability = $derived(cleanupAvailability(presentation.items));
  let observed = $derived(observedByteRange(scan?.total_bytes ?? 0, scan?.ambiguous_overlap_bytes));
  let isCurrent = $derived(scanStore.freshness === 'fresh' || scanStore.freshness === 'partial');
  let estimateLabel = $derived(scanStore.freshness === 'unavailable' ? 'Cleanup estimate unavailable'
    : scanStore.freshness === 'partial' ? 'Ready in checked locations'
    : isCurrent ? 'Ready to clean now' : 'Last ready-to-clean estimate');
</script>

<section class="storage-summary" aria-label="Storage scan summary">
  <div class="summary-ripples"><RippleScene /></div>
  <div class="summary-primary">
    <div class="flex flex-wrap items-center gap-2">
      <p class="text-meta font-medium text-muted-foreground">{scan ? estimateLabel : 'Available to clean'}</p>
      {#if scanStore.freshness === 'partial'}<Badge variant="outline">Partial scan</Badge>{/if}
    </div>
    <p class="mt-1 text-metric-lg font-mono font-semibold tracking-tight tabular-nums text-foreground">
      <AnimatedValue value={scan && presentation.hasMeasuredResults ? formatBytes(availability.ready) : '—'} />
    </p>
    {#if !scan || !presentation.hasMeasuredResults || !isCurrent}
      <p class="mt-1 text-meta text-muted-foreground">
        {#if !scan}Scan caches to see what can be cleaned.
        {:else if !presentation.hasMeasuredResults}Unread locations remain unknown.
        {:else}Scan again to verify these results.{/if}
      </p>
    {/if}

  </div>
  <div class="summary-context text-meta text-muted-foreground">
    <p>Found in scanned locations</p>
    <p class="mt-1 text-sm font-medium font-mono tabular-nums text-foreground">
      {#if !scan || !presentation.hasMeasuredResults}—
      {:else if observed.isAmbiguous}{formatBytes(observed.lower)}–{formatBytes(observed.upper)}
      {:else}{formatBytes(observed.upper)}{/if}
    </p>
    {#if observed.isAmbiguous && presentation.hasMeasuredResults}<p class="mt-1">Overlapping observations</p>{/if}
  </div>
    {#if isCurrent && presentation.hasMeasuredResults}
      <div class="summary-actions flex flex-wrap items-center gap-x-4 gap-y-1 text-meta text-muted-foreground">
        {#if availability.running > 0}
          <div class="flex flex-wrap items-center gap-1">
            <span>{formatBytes(availability.running)} requires idle apps</span>
            {#if onQuit}<Button variant="ghost" size="xs" disabled={!scanStore.canClean || actionsDisabled} onclick={onQuit}>Review apps…</Button>{/if}
          </div>
        {/if}
        {#if availability.review > 0}
          <div class="flex flex-wrap items-center gap-1">
            <span>{formatBytes(availability.review)} needs review</span>
            {#if onReview}<Button variant="ghost" size="xs" onclick={onReview}>Review items</Button>{/if}
          </div>
        {/if}
        {#if summary.unestimated_count > 0}
          <div class="flex flex-wrap items-center gap-1">
            <span>{summary.unestimated_count} {summary.unestimated_count === 1 ? 'action' : 'actions'} · Not estimated</span>
            {#if availability.review === 0 && onReview}<Button variant="ghost" size="xs" onclick={onReview}>Review items</Button>{/if}
          </div>
        {/if}
      </div>
    {/if}
</section>

<style>
  .storage-summary {
    position: relative;
    isolation: isolate;
    overflow: hidden;
    min-height: 188px;
    padding: 20px;
    border: 1px solid hsl(var(--border));
    border-radius: 16px;
    background: hsl(var(--card));
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr);
    align-items: center;
    column-gap: 24px;
    row-gap: 12px;
  }
  .summary-ripples { position: absolute; width: 320px; right: -48px; top: -8px; opacity: 0.28; z-index: -1; }
  .summary-primary { min-width: 0; }
  .summary-actions { grid-column: 1 / -1; border-top: 1px solid hsl(var(--border)); padding-top: 8px; }
  .summary-context { border-left: 1px solid hsl(var(--border)); padding-left: 24px; }
  @container (max-width: 460px) {
    .storage-summary { grid-template-columns: minmax(0, 1fr); gap: 12px; }
    .summary-context { border: 0; padding-left: 0; }
    .summary-ripples { display: none; }
  }
</style>
