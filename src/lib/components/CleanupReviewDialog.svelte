<script lang="ts">
  import { onMount } from 'svelte';
  import type { PlanPreview, ScanItem } from '../models/types';
  import { summarizeCleanupReview } from '../utils/cleanupReview';
  import { formatBytes } from '../utils/format';
  import Button from './Button.svelte';
  import { isFocusable, restoreFocus } from '../utils/focus';
  import { trapDialogFocus } from '../utils/modalDialog';
  import { platformContextStore } from '../stores/platformContext.svelte';

  let { plan, items = [], disabled = false, onCancel, onConfirm, returnFocusTarget }: {
    plan: PlanPreview;
    items?: ScanItem[];
    disabled?: boolean;
    onCancel: () => void;
    onConfirm: () => void;
    returnFocusTarget?: HTMLElement | null;
  } = $props();
  const id = $props.id();
  let dialog: HTMLDialogElement;
  let summary = $derived(summarizeCleanupReview(plan, items));
  let hasRebuild = $derived(summary.groups.some(group => !group.consequence
    && group.targets.some(target => target.risk === 'rebuild')));
  let actionSummary = $derived(plan.mode === 'trash'
    ? `Move to ${platformContextStore.trashLabel}`
    : plan.mode === 'mixed'
      ? `Some items move to ${platformContextStore.trashLabel}; others are deleted`
      : 'Delete permanently');
  let isConfirmed = false;

  onMount(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    if (typeof dialog?.showModal === 'function') {
      dialog.showModal();
      dialog.focus({ preventScroll: true });
    }
    return () => {
      if (dialog?.open) {
        dialog.close();
      }
      if (!isConfirmed) {
        const preferred = isFocusable(returnFocusTarget) ? returnFocusTarget : previousFocus;
        restoreFocus(preferred);
      }
    };
  });

  function handleConfirm() {
    isConfirmed = true;
    onConfirm();
  }

  function handleCancel() {
    onCancel();
  }
</script>

<dialog
  use:trapDialogFocus
  bind:this={dialog}
  id={id + '-dialog'}
  aria-modal="true"
  aria-labelledby={id + '-title'}
  aria-describedby={id + '-description'}
  tabindex="-1"
  oncancel={(event) => { event.preventDefault(); handleCancel(); }}
  class="m-auto w-[calc(100%-2rem)] max-w-lg max-h-[calc(100%-2rem)] overflow-hidden rounded-2xl border border-border bg-card p-0 text-foreground shadow-xl backdrop:bg-foreground/30 focus:outline-none open:flex open:flex-col"
>
  <header class="shrink-0 border-b border-border p-5">
  <h2 id={id + '-title'} class="text-title font-semibold tracking-tight">Clean {plan.targets.length} {plan.targets.length === 1 ? 'item' : 'items'}?</h2>
  <p id={id + '-description'} class="mt-2 text-body text-muted-foreground">
    {actionSummary}.
  </p>
  </header>
  <div class="min-h-0 overflow-y-auto scroll-stable px-5 py-4">
  <dl class="space-y-2 text-body">
    {#if plan.targets.some(target => target.mode === 'permanent_delete')}
      <div class="flex flex-wrap justify-between gap-2"><dt>Delete permanently</dt><dd class="font-mono tabular-nums">{summary.deletedBytes > 0 ? formatBytes(summary.deletedBytes) + ' estimated' : 'Not estimated'}</dd></div>
    {/if}
    {#if plan.targets.some(target => target.mode === 'trash')}
      <div class="flex flex-wrap justify-between gap-2"><dt>Move to {platformContextStore.trashLabel}</dt><dd class="font-mono tabular-nums">{summary.trashBytes > 0 ? formatBytes(summary.trashBytes) + ' estimated' : 'Not estimated'}</dd></div>
    {/if}
  </dl>
  {#if summary.unestimated > 0}
    <p class="mt-2 text-meta text-muted-foreground">{summary.unestimated} {summary.unestimated === 1 ? 'action has' : 'actions have'} no size estimate. The tool decides what it can remove; this is not included in the measured amounts.</p>
  {/if}
  {#if plan.targets.some(target => target.mode === 'trash')}
    <p class="mt-2 text-meta text-muted-foreground">Items in {platformContextStore.trashLabel} still use disk space until it is emptied.</p>
  {/if}
  {#if hasRebuild}
    <p class="mt-2 text-meta text-warning">Some items may download or build again later.</p>
  {/if}
  <ul class="mt-4 divide-y divide-border">
    {#each summary.groups as group}
      <li class="py-3 text-body">
        <div class="flex flex-wrap justify-between gap-2">
          <span class="min-w-0 break-words font-medium">{group.label}</span>
          <span class="font-mono tabular-nums">{group.bytes > 0 ? formatBytes(group.bytes) : 'Not estimated'}{group.bytes > 0 && group.unestimated > 0 ? ' + unestimated' : ''}</span>
        </div>
        <p class="mt-1 text-meta text-muted-foreground">{group.mode === 'trash' ? `Move to ${platformContextStore.trashLabel}` : group.mode === 'permanent_delete' ? 'Delete permanently' : actionSummary}</p>
        {#if group.consequence}<p class="mt-1 text-meta text-muted-foreground">{group.consequence}</p>{/if}
        <details class="mt-2">
          <summary class="w-fit cursor-pointer rounded text-meta text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring">View {group.targets.length} {group.targets.length === 1 ? 'item' : 'items'} and locations</summary>
          <ul class="mt-2 space-y-3 rounded-lg bg-secondary p-3">
            {#each group.targets as target (target.item_id)}
              <li class="min-w-0 text-meta">
                <div class="flex flex-wrap justify-between gap-2"><span class="break-words">{target.name}</span><span class="font-mono tabular-nums">{target.expected_bytes > 0 ? formatBytes(target.expected_bytes) : 'Not estimated'}</span></div>
                <code class="mt-1 block text-muted-foreground [overflow-wrap:anywhere]">{target.path}</code>
              </li>
            {/each}
          </ul>
        </details>
      </li>
    {/each}
  </ul>
  </div>
  <footer class="shrink-0 border-t border-border bg-card px-5 py-4">
  {#if disabled}
    <p role="status" class="mb-3 text-meta text-warning">This selection expired. Scan again.</p>
  {/if}
  <div class="flex flex-wrap justify-end gap-2">
    <Button variant="secondary" onclick={handleCancel}>Cancel</Button>
    <Button variant="destructive" disabled={disabled || plan.targets.length === 0} onclick={handleConfirm}>Clean items</Button>
  </div>
  </footer>
</dialog>
