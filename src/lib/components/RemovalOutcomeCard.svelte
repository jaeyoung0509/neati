<script lang="ts">
  import type { Snippet } from 'svelte';
  import { AlertCircle, CheckCircle2 } from '@lucide/svelte';
  import Card from './Card.svelte';
  import type { RemovalResult } from '../models/types';
  import { formatBytes } from '../utils/format';
  import { removalOutcome } from '../utils/removalOutcome';

  /**
   * The result of a reviewed permanent removal (#528). A run that removed
   * nothing leads with why, instead of a "0 removed" line over folded details.
   */
  let {
    result,
    singular,
    plural,
    sizeNote = '',
    children,
  }: {
    result: RemovalResult;
    singular: string;
    plural: string;
    sizeNote?: string;
    children?: Snippet;
  } = $props();

  const outcome = $derived(removalOutcome(result, singular, plural));
</script>

<Card class={`p-4 ${outcome.keptCount > 0 ? 'status-panel-warning' : 'status-panel-success'}`}>
  <div class="flex items-center justify-between gap-3 text-meta" role="status">
    <span class={`flex items-center gap-2 font-medium ${outcome.keptCount > 0 ? 'text-warning' : 'text-success'}`}>
      {#if outcome.keptCount > 0}
        <AlertCircle size={15} aria-hidden="true" />
      {:else}
        <CheckCircle2 size={15} aria-hidden="true" />
      {/if}
      {outcome.headline}
    </span>
    <span class="tabular-nums text-muted-foreground">{result.size_is_lower_bound ? '≥ ' : ''}{formatBytes(result.removed_allocated_size)} removed{sizeNote ? ` · ${sizeNote}` : ''}</span>
  </div>
  {@render children?.()}
  {#if outcome.reasons.length > 0}
    {#if outcome.nothingRemoved}
      <div class="mt-2 text-meta">
        <p class="text-muted-foreground">Why nothing was removed:</p>
        <ul class="mt-1 space-y-1 text-foreground">
          {#each outcome.reasons as reason (reason.message)}
            <li class="break-words">{reason.message}{reason.count > 1 ? ` (${reason.count})` : ''}</li>
          {/each}
        </ul>
      </div>
    {:else}
      <details class="mt-2 text-meta text-muted-foreground">
        <summary class="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">Why {outcome.keptCount === 1 ? 'one was' : `${outcome.keptCount} were`} kept</summary>
        <ul class="mt-1 space-y-1">
          {#each outcome.reasons as reason (reason.message)}
            <li class="break-words">{reason.message}{reason.count > 1 ? ` (${reason.count})` : ''}</li>
          {/each}
        </ul>
      </details>
    {/if}
  {/if}
</Card>
