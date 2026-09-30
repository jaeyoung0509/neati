<script lang="ts">
  import type { CategoryResult, DashboardTab, ScanResult } from '../models/types';
  import { cleanupGuidance, type CleanupGuidance } from '../utils/cleanup';
  import { formatBytes } from '../utils/format';
  import Button from './Button.svelte';
  interface Props {
    scan: ScanResult;
    onCategory?: (category: CategoryResult) => void;
    onNavigate?: (tab: DashboardTab) => void;
    onQuit?: () => void;
    current?: boolean;
    open?: boolean;
    summaryId?: string;
  }
  let { scan, onCategory, onNavigate, onQuit, current = false, open = $bindable(false), summaryId }: Props = $props();
  let rows = $derived(scan.categories.flatMap(category => category.items).map(cleanupGuidance)
    .filter(row => row.cause !== 'ready' || row.retainedBytes > 0)
    .sort((a, b) => b.observedBytes - a.observedBytes || a.name.localeCompare(b.name)));
  function hasOwnerAction(row: CleanupGuidance) {
    return row.action === 'review' || row.action === 'quit' || row.action === 'containers' || row.action === 'models';
  }
  let groups = $derived([
    { label: 'Review and owner actions', rows: rows.filter(hasOwnerAction) },
    { label: 'Kept and unverified observations', rows: rows.filter(row => !hasOwnerAction(row)) },
  ].filter(group => group.rows.length > 0));
  function act(row: CleanupGuidance) {
    if (row.action === 'quit' && onQuit) { onQuit(); return; }
    if (row.action === 'containers') { onNavigate?.('docker'); return; }
    if (row.action === 'models') { onNavigate?.('models'); return; }
    const category = scan.categories.find(category => category.category === row.category);
    if (category) onCategory?.(category);
  }
</script>

{#if rows.length > 0}
  <details bind:open class="text-meta text-muted-foreground" data-cleanup-item-details>
    <summary id={summaryId} class="w-fit cursor-pointer rounded-sm py-1 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary">Item details ({rows.length})</summary>
    {#if !current}<p class="mt-2">This is the last scan. Scan again before cleaning.</p>{/if}
    {#if (scan.ambiguous_overlap_bytes ?? 0) > 0}<p class="mt-2">Overlapping paths or shared file storage make row amounts upper bounds; do not add them as unique disk usage.</p>{/if}
    <div class="mt-2 max-h-80 overflow-y-auto rounded-xl border border-border bg-card">
      {#each groups as group (group.label)}
      <h3 class="border-b border-border bg-secondary px-3 py-2 font-medium text-foreground">{group.label} ({group.rows.length})</h3>
      <ul class="divide-y divide-border">
      {#each group.rows as row (row.itemId)}
        <li class="p-3 space-y-1">
          <div class="flex flex-wrap justify-between gap-2"><span class="font-medium text-foreground break-words">{row.name}</span><span class="font-mono tabular-nums">{formatBytes(row.observedBytes)} observed</span></div>
          <p>{row.owner} · {row.label}</p>
          <p class="break-words [overflow-wrap:anywhere]">{row.detail}</p>
          {#if row.action === 'unavailable'}
            <p class="font-medium">{row.actionLabel}</p>
          {:else}
            <Button variant="outline" size="sm" onclick={() => act(row)} ariaLabel={`${row.actionLabel}: ${row.name}`} disabled={row.action === 'quit' && !current}>{row.actionLabel}</Button>
          {/if}
        </li>
      {/each}
      </ul>
      {/each}
    </div>
  </details>
{/if}
