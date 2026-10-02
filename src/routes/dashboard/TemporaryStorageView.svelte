<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import Card from '../../lib/components/Card.svelte';
  import LoadingIndicator from '../../lib/components/LoadingIndicator.svelte';
  import { TemporaryReviewStore } from '../../lib/stores/temporaryReview.svelte';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import { formatBytes, formatTimeAgo } from '../../lib/utils/format';
  import { restoreFocus } from '../../lib/utils/focus';
  import { trapDialogFocus } from '../../lib/utils/modalDialog';
  import { isTauri } from '../../lib/utils/tauri';
  import type { TemporaryStorageInventory, TemporaryUsageState, TemporaryContentKind } from '../../lib/models/types';
  import { ArrowLeft, RefreshCw, X } from '@lucide/svelte';
  let { onBack, initialResult = null }: { onBack: () => void; initialResult?: TemporaryStorageInventory | null } = $props();
  const review = new TemporaryReviewStore(untrack(() => initialResult));
  let dialog = $state<HTMLDialogElement>();
  let acceptUnknown = $state(false);
  let acceptSource = $state(false);
  let previous: HTMLElement | null = null;
  const id = $props.id();
  const usageLabels: Record<TemporaryUsageState, string> = { in_use: 'In use', no_use_detected: 'No use detected', unable_to_determine: 'Unable to determine use' };
  const contentLabels: Record<TemporaryContentKind, string> = { build_output: 'Build output', source_checkout: 'Source checkout', git_metadata: 'Git / worktree metadata', browser_or_session_data: 'Browser or session data', unknown: 'Unclassified contents' };
  let options = $derived(review.inventory?.items.flatMap(item => item.options) ?? []);
  let selected = $derived(options.filter(option => review.selectedIds.includes(option.id)));
  let selectedBytes = $derived(selected.reduce((sum, option) => sum + (option.allocated_bytes ?? 0), 0));
  let unknownSelected = $derived(selected.filter(option => option.allocated_bytes === null || option.partial).length);
  const scopeCount = (count: number) => `${count} ${count === 1 ? 'scope' : 'scopes'}`;
  async function prepare() {
    previous = document.activeElement as HTMLElement | null;
    acceptUnknown = false; acceptSource = false;
    await review.prepare();
    if (review.preview) { await tick(); dialog?.showModal?.(); dialog?.focus(); }
  }
  function close() {
    if (review.busy === 'execute') { void review.stop(); return; }
    dialog?.close(); review.dismiss(); restoreFocus(previous);
  }
  onMount(() => { void platformContextStore.load(); });
  onDestroy(() => review.dispose());
</script>

<div class="space-y-4" data-testid="temporary-review">
  <div class="flex flex-wrap items-start gap-3">
    <Button variant="ghost" size="icon" ariaLabel="Back to Developer Artifacts" title="Back to Developer Artifacts" onclick={onBack}><ArrowLeft size={18} /></Button>
    <div class="min-w-0 flex-1 basis-48"><h1 class="text-title font-semibold">Temporary folders</h1><p class="mt-1 text-body text-muted-foreground">Review build directories, worktrees and scratch data. Nothing is preselected.</p></div>
    <Button disabled={!!review.busy} onclick={() => review.scan()} class="gap-1.5">{#if review.busy === 'scan'}<LoadingIndicator size="sm" />{:else}<RefreshCw size={14} />{/if}Scan temporary folders</Button>
  </div>
  {#if !isTauri()}<p role="status" class="text-meta text-muted-foreground">Browser fixture · native inspection and Trash moves require the desktop app.</p>{/if}
  <Card class="p-4 space-y-2">
    <p class="text-body"><strong>No use detected</strong> is a probe result. It does not prove abandonment or that source, Git or session data can be recovered.</p>
    <details class="text-meta text-muted-foreground"><summary class="cursor-pointer focus-ring">Scope and limits</summary><p class="mt-2">Only direct units under the platform’s user and shared temporary roots are inventoried. Age informs your decision. Access, current-user ownership, stable identity and no-follow boundaries remain required. Usage uncertainty can be accepted only in this review.</p>{#if review.inventory}<ul class="mt-2 space-y-1">{#each review.inventory.roots as root}<li class="break-all font-mono">{root}</li>{/each}</ul>{/if}</details>
  </Card>
  {#if review.error && !review.preview}<p role="alert" class="text-body text-destructive">{review.error}</p>{/if}
  {#if review.busy === 'scan'}<div role="status" class="flex items-center gap-3 text-body"><LoadingIndicator size="sm" />Inspecting temporary folders…<Button variant="outline" onclick={() => review.stop()}><X size={14} />Stop scan</Button></div>{/if}
  {#if review.inventory}
    {#if !review.inventory.available}<p role="status" class="text-body">{review.inventory.unavailable_reason}</p>
    {:else}
      <div class="flex flex-wrap items-end justify-between gap-3"><div><p class="text-meta text-muted-foreground">{review.result ? 'Previous review' : 'Observed allocations in temporary folders'}</p><p class="text-metric font-mono tabular-nums">{review.inventory.partial && !review.inventory.physical_overlap ? '≥ ' : ''}{formatBytes(review.inventory.observed_allocated_bytes)}</p><p class="text-meta text-muted-foreground">{review.inventory.items.length} folders · {review.inventory.unknown_estimates} partial or unknown estimates · separate from ready-to-clean cache totals</p>{#if review.inventory.physical_overlap}<p class="text-meta text-muted-foreground">Shared physical storage detected; distinct storage is unverified.</p>{/if}</div>{#if review.inventory.partial}<span class="rounded-md border border-border px-2 py-1 text-caption">Partial inventory</span>{/if}</div>
      <details class="text-meta text-muted-foreground"><summary class="cursor-pointer focus-ring">Inspection notes</summary><ul class="mt-2 space-y-1">{#each review.inventory.notes as note}<li>{note}</li>{/each}</ul></details>
      {#if review.inventory.items.length === 0}<Card class="p-6 text-body">{review.inventory.partial ? 'No measured units yet. Some locations could not be inspected.' : 'No temporary units were found in the inspected roots.'}</Card>{/if}
      <div class="overflow-hidden rounded-xl border border-border bg-card divide-y divide-border">
        {#each review.inventory.items as item (item.id)}
          <section class="p-4 space-y-2" aria-label={item.name}>
            <div class="flex items-start justify-between gap-3"><div class="min-w-0"><h2 class="text-body font-semibold break-all">{item.name}</h2><p class="mt-1 font-mono text-caption text-muted-foreground break-all">{item.path}</p></div><p class="shrink-0 font-mono text-body tabular-nums">{item.allocated_bytes === null ? 'Not estimated' : `${item.partial ? '≥ ' : ''}${formatBytes(item.allocated_bytes)}`}</p></div>
            <p class="text-meta"><strong>{usageLabels[item.usage.state]}</strong> · {item.contents.map(kind => contentLabels[kind]).join(' · ')} · {item.newest_activity ? formatTimeAgo(item.newest_activity) : 'Latest activity unavailable'}</p>
            <div class="space-y-2">{#each item.options as option}<label class="flex items-start gap-2 text-body"><input type="checkbox" class="mt-1 accent-success focus-ring" checked={review.selectedIds.includes(option.id)} disabled={!!option.blocked_reason || !!review.busy || !!review.result} onchange={event => review.choose(item.id, option.id, event.currentTarget.checked)} /><span class="min-w-0">{option.mode === 'whole_folder' ? 'Entire temporary folder · may include source and Git data' : 'Generated subtree only'}{#if option.mode === 'generated_subtree'}<span class="block break-all font-mono text-caption text-muted-foreground">{option.path} · {option.allocated_bytes === null ? 'Not estimated' : formatBytes(option.allocated_bytes)}</span>{/if}{#if option.blocked_reason}<span class="block text-meta text-muted-foreground">{option.blocked_reason}</span>{/if}</span></label>{/each}</div>
            <details class="text-meta text-muted-foreground"><summary class="cursor-pointer focus-ring">Use evidence and limitations</summary><p class="mt-2">{item.usage.probe} · observed {new Date(item.usage.observed_at * 1000).toLocaleTimeString()}</p><ul class="mt-1 space-y-1">{#each item.usage.evidence as evidence}<li class="break-all">{evidence}</li>{/each}</ul><p class="mt-2">{item.usage.limitation}</p></details>
          </section>
        {/each}
      </div>
      {#if review.result}<p role="status" class="text-body">{isTauri() ? `Moved ${review.result.moved_count} units to ${platformContextStore.trashLabel}` : `Previewed ${review.result.moved_count} moves; no files changed`} · {review.result.skipped_count + review.result.failed_count} kept. Scan again to refresh.</p>{/if}
      {#if selected.length}<div class="sticky bottom-0 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-card p-4"><p class="text-body">{scopeCount(selected.length)} selected · {formatBytes(selectedBytes)} known bytes{unknownSelected ? ` · ${unknownSelected} partial or unknown estimates` : ''}</p><Button disabled={!!review.busy} onclick={prepare}>{review.busy === 'prepare' ? 'Preparing review…' : 'Review selected'}</Button></div>{/if}
    {/if}
  {:else}<Card class="p-6 text-body text-muted-foreground">Scan the stated temporary roots to review the largest units first.</Card>{/if}
</div>

{#if review.preview}
  <dialog use:trapDialogFocus bind:this={dialog} tabindex="-1" aria-labelledby={id + '-title'} aria-describedby={id + '-description'} oncancel={event => { event.preventDefault(); close(); }} class="m-auto w-[calc(100%-2rem)] max-w-xl max-h-[calc(100%-2rem)] overflow-y-auto rounded-2xl border border-border bg-card p-5 text-foreground shadow-xl backdrop:bg-foreground/30">
    <h2 id={id + '-title'} class="text-title font-semibold">{review.result ? 'Temporary review result' : 'Move these exact scopes to Trash?'}</h2>
    <p id={id + '-description'} class="mt-2 text-body">{scopeCount(review.preview.selected.length)} · {formatBytes(review.preview.known_allocated_bytes)} known bytes · {review.preview.unknown_estimates} partial or unknown estimates. Trash movement does not establish immediate free-space recovery.</p>
    <ul class="mt-3 space-y-2 text-meta">{#each review.preview.selected as option}<li><strong>{option.mode === 'whole_folder' ? 'Whole folder' : 'Generated subtree'}</strong><span class="block font-mono break-all">{option.path}</span></li>{/each}</ul>
    <ul class="mt-3 space-y-2 text-body text-muted-foreground">{#each review.preview.warnings as warning}<li>{warning}</li>{/each}</ul>
    {#if !review.result}
      {#if review.preview.has_unknown_usage}<label class="mt-4 flex items-start gap-2 text-body"><input type="checkbox" bind:checked={acceptUnknown} disabled={review.busy === 'execute'} class="mt-1 accent-success focus-ring" /><span>I accept that use could not be determined for these reviewed temporary scopes.</span></label>{/if}
      {#if review.preview.has_whole_folders}<label class="mt-3 flex items-start gap-2 text-body"><input type="checkbox" bind:checked={acceptSource} disabled={review.busy === 'execute'} class="mt-1 accent-success focus-ring" /><span>I understand that source, Git/worktree metadata, unpublished work and session data may be lost from whole folders.</span></label>{/if}
      <p class="mt-3 text-caption text-muted-foreground">One-shot review expires at {new Date(review.preview.expires_at * 1000).toLocaleTimeString()}. New active use or changed contents blocks that unit.</p>
    {:else}<ul class="mt-3 space-y-2 text-body">{#each review.result.items as item}<li class="break-all">{item.message}</li>{/each}</ul>{/if}
    {#if review.error}<p role="alert" class="mt-3 text-body text-destructive">{review.error}</p>{/if}
    <div class="sticky bottom-0 mt-5 flex justify-end gap-2 bg-card py-2"><Button variant="outline" onclick={close}>{review.busy === 'execute' ? 'Stop' : review.result ? 'Done' : 'Cancel'}</Button>{#if !review.result}<Button variant="destructive" disabled={!!review.busy || (review.preview.has_unknown_usage && !acceptUnknown) || (review.preview.has_whole_folders && !acceptSource) || !!review.error} onclick={() => review.execute(acceptUnknown, acceptSource)}>{review.busy === 'execute' ? 'Moving reviewed scopes…' : isTauri() ? 'Move reviewed scopes to Trash' : 'Preview reviewed moves'}</Button>{/if}</div>
  </dialog>
{/if}
