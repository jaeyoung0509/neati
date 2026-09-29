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
  }
  let { scan, onCategory, onNavigate, onQuit, current = false }: Props = $props();
  let rows = $derived(scan.categories.flatMap(category => category.items).map(cleanupGuidance)
    .filter(row => row.cause !== 'ready' || row.retainedBytes > 0)
    .sort((a, b) => b.observedBytes - a.observedBytes || a.name.localeCompare(b.name)));
  function act(row: CleanupGuidance) {
    if (row.action === 'quit' && onQuit) { onQuit(); return; }
    if (row.action === 'containers') { onNavigate?.('docker'); return; }
    if (row.action === 'models') { onNavigate?.('models'); return; }
    const category = scan.categories.find(category => category.category === row.category);
    if (category) onCategory?.(category);
  }
</script>

{#if rows.length > 0}
  <details class="mt-3 text-meta text-muted-foreground">
    <summary class="cursor-pointer rounded-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary">Items and next actions ({rows.length})</summary>
    {#if !current}<p class="mt-2">This is the last scan. Scan again before cleaning.</p>{/if}
    {#if (scan.ambiguous_overlap_bytes ?? 0) > 0}<p class="mt-2">Overlapping paths or shared file storage make row amounts upper bounds; do not add them as unique disk usage.</p>{/if}
    <ul class="mt-2 max-h-80 overflow-y-auto divide-y divide-border rounded-xl border border-border bg-card">
      {#each rows as row (row.itemId)}
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
  </details>
{/if}
