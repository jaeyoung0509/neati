<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import Button from './Button.svelte';
  import { restoreFocus } from '../utils/focus';
  import { trapDialogFocus } from '../utils/modalDialog';
  import { formatBytes } from '../utils/format';
  import { emptyReviewedTrash } from '../utils/emptyTrash';
  import { tauriPreviewEmptyTrash, tauriCancelEmptyTrash } from '../utils/tauri';
  import type { EmptyTrashPreview, EmptyTrashResult } from '../models/types';
  let { disabled = false }: { disabled?: boolean } = $props();
  let preview = $state<EmptyTrashPreview | null>(null);
  let result = $state<EmptyTrashResult | null>(null);
  let error = $state('');
  let busy = $state(false);
  let loading = $state(false);
  let dialog = $state<HTMLDialogElement>();
  let previous: HTMLElement | null = null;
  let disposed = false;
  const id = $props.id();
  async function review() {
    previous = document.activeElement as HTMLElement | null;
    loading = true; error = ''; result = null;
    try {
      const next = await tauriPreviewEmptyTrash();
      if (disposed) { void tauriCancelEmptyTrash(next.plan_id).catch(() => {}); return; }
      preview = next;
      await tick(); dialog?.showModal?.(); dialog?.focus();
    } catch (cause) { error = String(cause); }
    finally { loading = false; }
  }
  async function execute() {
    if (!preview || busy) return;
    busy = true; error = '';
    try { result = await emptyReviewedTrash(preview); }
    catch (cause) { error = String(cause); }
    finally { busy = false; }
  }
  function close() {
    if (preview) void tauriCancelEmptyTrash(preview.plan_id).catch(cause => { error = String(cause); });
    if (busy) return;
    dialog?.close(); preview = null; result = null; restoreFocus(previous);
  }
  onDestroy(() => { disposed = true; if (preview) void tauriCancelEmptyTrash(preview.plan_id).catch(() => {}); });
</script>
<Button variant="outline" size="sm" disabled={disabled || loading} onclick={review}>{loading ? 'Reading Trash…' : 'Review Trash'}</Button>
{#if error && !preview}<span role="alert" class="text-meta text-destructive">{error}</span>{/if}
{#if preview}
  <dialog use:trapDialogFocus bind:this={dialog} tabindex="-1" aria-labelledby={id + '-title'} aria-describedby={id + '-description'}
    oncancel={event => { event.preventDefault(); close(); }}
    class="m-auto w-[calc(100%-2rem)] max-w-lg max-h-[calc(100%-2rem)] overflow-y-auto rounded-2xl border border-border bg-card p-5 text-foreground shadow-xl backdrop:bg-foreground/30">
    <h2 id={id + '-title'} class="text-title font-semibold">{result ? 'Trash cleanup result' : 'Empty home Trash?'}</h2>
    <p id={id + '-description'} class="mt-2 text-body text-muted-foreground">This permanently removes the reviewed items. You cannot put them back afterward. Other volumes’ Trash stays intact.</p>
    <p class="mt-3 break-all text-meta text-muted-foreground">{preview.scope}</p>
    {#if result}
      <p role="status" class="mt-3 text-body">{result.removed_entries} entries removed · {formatBytes(result.removed_bytes)} removed file data{result.cancelled ? ' · Stopped before completion' : ''}</p>
      <p class="mt-2 text-meta text-muted-foreground">Removed file data is not a measurement of increased free disk space. Scan again to refresh storage totals.</p>
      <details class="mt-3 text-meta"><summary>Per-item results</summary>
        <ul class="mt-2 space-y-1">{#each result.items as item}<li class="break-all">{item.path}: {item.message}</li>{/each}</ul>
      </details>
    {:else}
      <p class="mt-3 text-body">{preview.entry_count} entries · {formatBytes(preview.observed_bytes)} observed</p>
      <details class="mt-3 text-meta"><summary>Review items</summary>
        <ul class="mt-2 space-y-1">{#each preview.items as item}<li class="break-all">{item}</li>{/each}</ul>
      </details>
    {/if}
    {#if error}<p role="alert" class="mt-3 text-body text-destructive">{error}</p>{/if}
    {#if busy}<p role="status" class="mt-3 text-body">Emptying reviewed items… Stop preserves entries that have not been removed yet.</p>{/if}
    <div class="sticky bottom-0 mt-5 flex justify-end gap-2 bg-card py-2">
      <Button variant="outline" onclick={close}>{busy ? 'Stop' : result ? 'Done' : 'Cancel'}</Button>
      {#if !result}<Button variant="destructive" disabled={busy || preview.entry_count === 0 || !!error} onclick={execute}>Empty reviewed Trash</Button>{/if}
    </div>
  </dialog>
{/if}
