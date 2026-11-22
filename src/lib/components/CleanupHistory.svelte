<script lang="ts">
  import { onMount } from 'svelte';
  import type { CleanupHistoryEntry } from '../models/types';
  import { tauriGetCleanupHistory } from '../utils/tauri';
  import { cleanupHistoryDate, cleanupHistoryIssues, cleanupHistoryTotals } from '../utils/cleanupHistory';
  import { formatBytes } from '../utils/format';
  import Button from './Button.svelte';

  const VISIBLE = 5;
  let entries = $state<CleanupHistoryEntry[] | null>(null);
  let error = $state('');
  let showAll = $state(false);
  let totals = $derived(cleanupHistoryTotals(entries ?? []));
  let visible = $derived(showAll ? (entries ?? []) : (entries ?? []).slice(0, VISIBLE));

  async function load() {
    error = '';
    try {
      entries = await tauriGetCleanupHistory();
    } catch (cause) {
      entries = null;
      error = String(cause);
    }
  }
  onMount(() => { void load(); });
</script>

<div class="space-y-3 text-meta" data-cleanup-history>
  {#if error}
    <p role="alert" class="text-destructive">{error}</p>
    <Button size="sm" variant="outline" onclick={() => void load()}>Try again</Button>
  {:else if entries === null}
    <p role="status" class="text-muted-foreground">Reading cleanup history…</p>
  {:else if entries.length === 0}
    <p class="text-muted-foreground">No cleanups yet. Completed cleanups are listed here as totals.</p>
  {:else}
    <p class="text-muted-foreground">
      <span class="font-mono tabular-nums text-foreground">{formatBytes(totals.removedBytes)}</span>
      removed across {totals.runs} {totals.runs === 1 ? 'cleanup' : 'cleanups'}. Removed data is not a measured change in free disk space.
    </p>
    <ul class="divide-y divide-border rounded-xl border border-border">
      {#each visible as entry (entry.finished_at + ':' + entry.item_count + ':' + entry.removed_bytes)}
        {@const issues = cleanupHistoryIssues(entry)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <div class="min-w-0">
            <div class="text-foreground">{cleanupHistoryDate(entry)}</div>
            <div class="text-muted-foreground">
              {entry.item_count} {entry.item_count === 1 ? 'item' : 'items'}{#if issues}{' · '}<span class="text-warning">{issues}</span>{/if}
            </div>
          </div>
          <span class="shrink-0 font-mono tabular-nums text-foreground">{formatBytes(entry.removed_bytes)}</span>
        </li>
      {/each}
    </ul>
    {#if entries.length > VISIBLE}
      <Button size="sm" variant="ghost" aria-expanded={showAll} onclick={() => (showAll = !showAll)}>
        {showAll ? 'Show fewer' : `Show all ${entries.length}`}
      </Button>
    {/if}
    <p class="text-caption text-muted-foreground">Only totals are stored: no file names or locations. The newest 200 cleanups are kept.</p>
  {/if}
</div>
