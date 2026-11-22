<script lang="ts">
  import { Hammer } from '@lucide/svelte';
  import Button from './Button.svelte';
  import Checkbox from './Checkbox.svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';
  import { projectCleanupStore } from '../stores/projectCleanup.svelte';
  import { formatBytes } from '../utils/format';
  import { developerArtifactKindLabel } from '../utils/developerArtifactKinds';

  /**
   * Project build output as one more row of Clean (#494): idle, verified
   * compiler output from saved project folders, removed by the same Clean
   * action as caches. Everything else stays in Projects for review.
   */
  let { disabled = false, onOpenProjects }: {
    disabled?: boolean;
    onOpenProjects?: () => void;
  } = $props();

  let store = projectCleanupStore;
  let items = $derived(store.items);
  let selected = $derived(new Set(store.selectedIds));
  let allSelected = $derived(items.length > 0 && items.every((item) => selected.has(item.id)));
  let someSelected = $derived(items.some((item) => selected.has(item.id)));
  let totalBytes = $derived(items.reduce((total, item) => total + item.allocated_bytes, 0));

  function toggleAll(checked: boolean) {
    for (const item of items) store.toggle(item.id, checked);
  }
</script>

<div class="category-row" data-project-build-output>
  <div class="flex min-w-0 items-center gap-3">
    {#if store.phase === 'ready' && items.length > 0}
      <Checkbox
        class="h-8 w-8 shrink-0"
        checked={allSelected}
        indeterminate={!allSelected && someSelected}
        disabled={disabled}
        onchange={toggleAll}
        ariaLabel="Select all project build output"
      />
    {:else}
      <div class="flex h-8 w-8 shrink-0 items-center justify-center text-meta text-muted-foreground" aria-hidden="true">-</div>
    {/if}
    <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-secondary text-muted-foreground" aria-hidden="true">
      <Hammer size={16} />
    </div>
    <div class="min-w-0 flex-1">
      <p class="text-body font-medium text-foreground">Project build output</p>
      {#if !store.hasSavedFolders}
        <p class="text-meta text-muted-foreground">Not scanned · add your projects folder once and Clean includes idle build output.</p>
      {:else if store.phase === 'scanning'}
        <p class="flex items-center gap-1.5 text-meta text-muted-foreground" role="status" aria-live="polite">
          <LoadingIndicator size="xs" word="scanning" class="text-primary" />
          <span class="truncate">{store.activity || 'Scanning project folders…'}</span>
        </p>
      {:else if store.phase === 'cleaning'}
        <p class="flex items-center gap-1.5 text-meta text-muted-foreground" role="status" aria-live="polite">
          <LoadingIndicator size="xs" word="cleaning" class="text-primary" />
          <span>Removing build output…</span>
        </p>
      {:else if store.phase === 'error'}
        <p class="text-meta text-destructive" role="alert">{store.error}</p>
      {:else if items.length === 0}
        <p class="text-meta text-muted-foreground">No idle build output in your project folders.</p>
      {:else}
        <p class="text-meta text-muted-foreground">{items.length} {items.length === 1 ? 'folder' : 'folders'} · rebuilt by the next build</p>
      {/if}
    </div>
    {#if store.phase === 'ready' && items.length > 0}
      <span class="shrink-0 font-mono text-body font-semibold tabular-nums text-foreground">{formatBytes(totalBytes)}</span>
    {:else if !store.hasSavedFolders || items.length === 0}
      <Button variant="ghost" size="sm" class="shrink-0" onclick={() => onOpenProjects?.()}>
        {store.hasSavedFolders ? 'Review in Projects' : 'Add folder'}
      </Button>
    {/if}
  </div>
  {#if store.phase === 'ready' && items.length > 0}
    <ul class="mt-2 space-y-1 pl-11" aria-label="Project build output">
      {#each items as item (item.id)}
        <li class="flex items-center gap-3">
          <Checkbox
            class="h-7 w-7 shrink-0"
            checked={selected.has(item.id)}
            disabled={disabled}
            onchange={(checked) => store.toggle(item.id, checked)}
            ariaLabel={`Select ${item.project_name} ${developerArtifactKindLabel(item.kind)}`}
          />
          <div class="min-w-0 flex-1">
            <p class="truncate text-meta font-medium text-foreground">{item.project_name} · {developerArtifactKindLabel(item.kind)}</p>
            <p class="truncate font-mono text-caption text-muted-foreground">{item.path}</p>
          </div>
          <span class="shrink-0 font-mono text-meta tabular-nums text-muted-foreground">{formatBytes(item.allocated_bytes)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
