<script lang="ts">
  import type { AppLeftoverClassification, AppLeftoverInventory } from '../models/types';
  import Button from './Button.svelte';
  import Checkbox from './Checkbox.svelte';
  import InlineNotice from './InlineNotice.svelte';
  import { FolderOpen, RefreshCw } from '@lucide/svelte';
  import { formatBytes } from '../utils/format';
  import { tauriGetAppLeftovers, tauriShowInFileManager } from '../utils/tauri';
  import { canReveal, revealUnavailableReason, runReveal } from '../utils/reveal';
  import { platformContextStore } from '../stores/platformContext.svelte';

  interface Props { initialInventory?: AppLeftoverInventory | null; initialOpen?: boolean; }
  let { initialInventory = null, initialOpen = false }: Props = $props();
  // svelte-ignore state_referenced_locally
  let inventory = $state<AppLeftoverInventory | null>(initialInventory);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let revealError = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  let opened = $state(initialOpen);
  let showInstalled = $state(false);
  let visibleItems = $derived(inventory?.items.filter(item => showInstalled || item.classification !== 'installed_owner') ?? []);
  const labels: Record<AppLeftoverClassification, string> = {
    installed_owner: 'Installed owner found',
    possible_removed_owner: 'Possible removed owner',
    ambiguous_shared_owner: 'Owner uncertain or shared',
    incomplete_inventory: 'Owner check incomplete',
    protected_state: 'Protected state',
  };
  async function refresh() {
    if (loading) return;
    loading = true;
    error = null;
    try { inventory = await tauriGetAppLeftovers(); }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { loading = false; }
  }
</script>

<details open={opened} class="rounded-xl border border-border bg-card" ontoggle={(event) => {
  opened = event.currentTarget.open;
  if (opened && !inventory && !loading && !error) void refresh();
}}>
  <summary class="cursor-pointer px-4 py-3 text-sm font-medium focus-ring">
    Review possible app leftovers
    <span class="ml-2 text-caption font-normal text-muted-foreground">Read-only</span>
  </summary>
  {#if opened}
    <div class="space-y-3 border-t border-border px-4 pb-4 pt-3">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <p class="max-w-prose text-meta text-muted-foreground">Inspect named user Library resources and their possible owners.</p>
        <Button variant="outline" size="sm" onclick={() => void refresh()} disabled={loading}>
          <RefreshCw size={14} aria-hidden="true" />{loading ? 'Inspecting…' : 'Refresh review'}
        </Button>
      </div>
      {#if error}<InlineNotice variant="error" title="Resource review unavailable" message={error} />{/if}
      {#if revealError}<InlineNotice variant="error" title="Could not reveal resource" message={revealError} />{/if}
      {#if loading}<p class="text-meta text-muted-foreground" role="status">Inspecting named user Library locations and application identities…</p>{/if}
      {#if inventory}
        {#if loading || error}<p class="text-meta text-muted-foreground">Showing the previous observation; a fresh review has not completed.</p>{/if}
        <p class="text-meta text-muted-foreground">{inventory.limitation}</p>
        {#if inventory.quality !== 'fresh'}
          <InlineNotice variant="info" title="Partial resource review" message={inventory.incomplete_reasons[0] ?? 'Some owners or locations could not be checked. Unknown locations are not counted as empty.'} />
        {/if}
        <Checkbox checked={showInstalled} onchange={(checked) => { showInstalled = checked; }}
          ariaLabel="Include resources with an installed owner" label="Include resources with an installed owner"
          class="w-fit justify-start gap-2 text-muted-foreground" />
        <p class="text-caption text-muted-foreground">{visibleItems.length} observed resource{visibleItems.length === 1 ? '' : 's'} · {inventory.observed_roots} checked locations · separate from Cleanup estimates</p>
        {#if visibleItems.length === 0}
          <p class="rounded-lg bg-secondary p-3 text-meta text-muted-foreground">{inventory.quality === 'fresh' ? 'No possible leftovers were identified in the checked locations. Other locations or owners may be outside this bounded review.' : 'No resource rows are available from this partial review. Unchecked locations and owners remain unknown.'}</p>
        {:else}
          <ul class="divide-y divide-border rounded-lg border border-border" aria-label="Read-only application resource inventory">
            {#each visibleItems as item (item.id)}
              <li class="space-y-2 px-3 py-3">
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0">
                    <p class="break-words text-sm font-medium">{item.name}</p>
                    <p class="mt-1 text-caption text-muted-foreground">{labels[item.classification]} · cleanup unavailable</p>
                  </div>
                  <span class="shrink-0 whitespace-nowrap text-meta font-mono tabular-nums">
                    {item.quality === 'unavailable' ? 'Unknown size' : `${item.quality === 'partial' ? 'At least ' : ''}${formatBytes(item.allocated_size)}`}
                  </span>
                </div>
                <details class="text-meta">
                  <summary class="w-fit cursor-pointer text-muted-foreground focus-ring">Evidence and location</summary>
                  <div class="mt-2 space-y-2">
                    <p class="max-w-prose text-muted-foreground">{item.evidence}</p>
                    {#if item.owner_names.length}<p class="text-muted-foreground">Matched owners: {item.owner_names.join(', ')}</p>{/if}
                    {#if item.incomplete_reason}<p class="text-muted-foreground">{item.incomplete_reason}</p>{/if}
                    <p class="break-all font-mono text-caption text-muted-foreground">{item.display_path}</p>
                    <Button variant="ghost" size="sm" disabled={!canReveal()} title={!canReveal() ? revealUnavailableReason() : undefined}
                      onclick={() => void runReveal(() => tauriShowInFileManager(item.display_path), message => revealError = message)}>
                      <FolderOpen size={14} aria-hidden="true" />{platformContextStore.revealLabel}
                    </Button>
                  </div>
                </details>
              </li>
            {/each}
          </ul>
        {/if}
        {#if inventory.incomplete_reasons.length > 1}
          <details class="text-meta text-muted-foreground"><summary class="cursor-pointer focus-ring">Inspection limits</summary>
            <ul class="mt-2 list-disc space-y-1 pl-4">{#each inventory.incomplete_reasons as reason}<li>{reason}</li>{/each}</ul>
          </details>
        {/if}
      {/if}
    </div>
  {/if}
</details>
