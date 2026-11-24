<script lang="ts">
  import { onMount } from 'svelte';
  import type { ScanItem } from '../models/types';
  import { cleanableBytes } from '../utils/cleanup';
  import { formatBytes } from '../utils/format';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  interface Props {
    items: ScanItem[];
    partial: boolean;
    disabled?: boolean;
    onCancel: () => void;
    onConfirm: (selectedItemIds: string[]) => void;
  }

  let { items, partial, disabled = false, onCancel, onConfirm }: Props = $props();
  const id = $props.id();
  let confirmed = false;
  let selected = $state<Record<string, boolean>>({});
  let selectedItems = $derived(items.filter((item) => selected[item.id]));
  let selectedBytes = $derived(selectedItems.reduce((total, item) => total + cleanableBytes(item), 0));

  onMount(() => {
    selected = Object.fromEntries(items.map((item) => [item.id, true]));
  });

  function confirm() {
    if (disabled || selectedItems.length === 0) return;
    confirmed = true;
    onConfirm(selectedItems.map((item) => item.id));
  }
</script>

<Dialog
  {onCancel}
  restoreFocusOnClose={() => !confirmed}
  size="sm"
  density="compact"
  id={id + '-dialog'}
  aria-labelledby={id + '-title'}
  aria-describedby={id + '-description'}
>
  <h2 id={id + '-title'} class="text-body font-semibold tracking-tight">Review Safe cleanup</h2>
  <p id={id + '-description'} class="mt-1 text-meta leading-snug text-muted-foreground">
    {partial
      ? 'This scan missed some locations. Only the measured Safe items below can be cleaned.'
      : 'Only verified Safe items are included. Other cleanup stays in Clean.'}
  </p>
  <div class="mt-3 flex items-baseline justify-between gap-2 border-b border-border pb-2 text-meta">
    <span class="text-muted-foreground">{selectedItems.length} of {items.length} selected</span>
    <strong class="font-semibold tabular-nums">{formatBytes(selectedBytes)}</strong>
  </div>
  <ul class="max-h-64 overflow-y-auto scroll-stable divide-y divide-border" aria-label="Safe cleanup items">
    {#each items as item (item.id)}
      <li>
        <label class="flex cursor-pointer items-start gap-2.5 rounded-md px-1 py-2 hover:bg-accent/50">
          <input
            type="checkbox"
            checked={selected[item.id]}
            disabled={disabled}
            onchange={(event) => (selected[item.id] = event.currentTarget.checked)}
            class="mt-0.5 size-4 shrink-0 accent-control focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-meta font-medium" title={item.name}>{item.name}</span>
            <span class="block truncate font-mono text-micro text-muted-foreground" title={item.path}>{item.path}</span>
          </span>
          <span class="shrink-0 text-meta font-medium tabular-nums">{formatBytes(cleanableBytes(item))}</span>
        </label>
      </li>
    {/each}
  </ul>
  {#if disabled}
    <p class="mt-2 text-meta text-warning" role="status">The scan changed. Scan again before cleaning.</p>
  {/if}
  <div class="mt-4 flex justify-end gap-2">
    <Button variant="secondary" size="sm" onclick={onCancel}>Cancel</Button>
    <Button variant="destructive" size="sm" disabled={disabled || selectedItems.length === 0} onclick={confirm}>
      Clean selected
    </Button>
  </div>
</Dialog>
