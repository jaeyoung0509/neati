<script lang="ts">
  import { onMount } from 'svelte';
  import type { CleanupQuitPreview } from '../models/types';
  import Button from './Button.svelte';
  import { restoreFocus } from '../utils/focus';
  import { trapDialogFocus } from '../utils/modalDialog';
  let { preview, busy, progress, onConfirm, onCancel }: {
    preview: CleanupQuitPreview; busy: boolean; progress: string;
    onConfirm: () => void; onCancel: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  const id = $props.id();
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.showModal?.();
    dialog.focus();
    return () => { dialog.close?.(); restoreFocus(previous); };
  });
</script>

<dialog use:trapDialogFocus bind:this={dialog} tabindex="-1" aria-modal="true" aria-labelledby={id + '-title'}
  aria-describedby={id + '-description'}
  oncancel={(event) => { event.preventDefault(); onCancel(); }}
  class="m-auto w-[calc(100%-2rem)] max-w-lg max-h-[calc(100%-2rem)] overflow-y-auto rounded-2xl border border-border bg-card p-5 text-foreground shadow-xl backdrop:bg-foreground/30">
  <h2 id={id + '-title'} class="text-title font-semibold">Quit apps and check caches?</h2>
  <p id={id + '-description'} class="mt-2 text-body text-muted-foreground">
    Save your work first. These apps will receive a quit request. neati will scan again and review the newly available caches before cleaning.
  </p>
  <ul class="my-4 space-y-1 text-body">
    {#each preview.apps as app (app.lease_id)}<li>{app.name}</li>{/each}
  </ul>
  {#if preview.unavailable.length > 0}
    <p class="text-meta text-muted-foreground">No verified quit action for: {preview.unavailable.join(', ')}. Wait until these caches are idle, then scan again.</p>
  {/if}
  {#if progress}<p role="status" class="mt-3 text-body">{progress}</p>{/if}
  {#if busy}<p class="mt-2 text-meta text-muted-foreground">Cancel stops the next step. Apps already asked to quit may close.</p>{/if}
  <div class="mt-5 flex justify-end gap-2">
    <Button variant="outline" onclick={onCancel}>{busy ? 'Stop after current step' : 'Cancel'}</Button>
    <Button disabled={busy || preview.apps.length === 0} onclick={onConfirm}>Quit and check caches</Button>
  </div>
</dialog>
