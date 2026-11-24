<script lang="ts">
  import { AlertCircle, AlertTriangle, CheckCircle2, X } from '@lucide/svelte';
  import type { OneButtonOutcome } from '../models/types';
  import { formatBytes } from '../utils/format';
  import { oneButtonOutcomeState } from '../utils/oneButtonClean';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  /** The result of one-button Clean (#500): caches and project build output together. */
  interface Props {
    outcome: OneButtonOutcome;
    onClose: () => void;
    returnFocusTargetId?: string;
  }

  let { outcome, onClose, returnFocusTargetId }: Props = $props();
  const id = $props.id();
  let resultState = $derived(oneButtonOutcomeState(outcome));
</script>

<Dialog
  onCancel={onClose}
  size="sm"
  density="compact"
  aria-labelledby={id + '-title'}
  aria-describedby={id + '-description'}
  returnFocusTarget={() => (returnFocusTargetId ? document.getElementById(returnFocusTargetId) : null)}
>
  <div class="flex items-start justify-between gap-3">
    <div class="flex items-start gap-3">
      <div
        class={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full ${
          resultState === 'success' ? 'bg-success/20 text-success' : resultState === 'partial' ? 'bg-warning/20 text-warning' : 'bg-destructive/20 text-destructive'
        }`}
        aria-hidden="true"
      >
        {#if resultState === 'success'}
          <CheckCircle2 size={18} />
        {:else if resultState === 'partial'}
          <AlertTriangle size={18} />
        {:else}
          <AlertCircle size={18} />
        {/if}
      </div>
      <div>
        <h3 id={id + '-title'} class="text-title font-semibold tracking-tight text-foreground">
          {resultState === 'success' ? 'Cleanup complete' : resultState === 'partial' ? 'Some items could not be cleaned' : 'Cleanup failed'}
        </h3>
        <p id={id + '-description'} class="text-meta text-muted-foreground">
          {outcome.removed} {outcome.removed === 1 ? 'item' : 'items'} permanently removed{outcome.skipped > 0 ? ` · ${outcome.skipped} already gone` : ''}
        </p>
      </div>
    </div>
    <Button variant="ghost" size="icon" onclick={onClose} aria-label="Close cleanup result" title="Close">
      <X size={16} />
    </Button>
  </div>

  <div class="space-y-3 py-3">
    <div class="rounded-xl bg-secondary p-3 text-center">
      <div class="text-metric font-semibold tabular-nums text-foreground">{formatBytes(outcome.reclaimed_bytes)}</div>
      <div class="mt-0.5 text-meta text-muted-foreground">Removed from storage</div>
    </div>
    {#if outcome.failed.length > 0}
      <ul class="space-y-1.5" aria-label="Items that were not removed">
        {#each outcome.failed as failure (failure.reference)}
          <li class="rounded-lg bg-destructive/10 px-2.5 py-1.5 text-meta">
            <p class="font-medium text-foreground">{failure.name}</p>
            <p class="text-muted-foreground">{failure.message}</p>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="flex justify-end">
    <Button variant="primary" size="sm" onclick={onClose}>Done</Button>
  </div>
</Dialog>
