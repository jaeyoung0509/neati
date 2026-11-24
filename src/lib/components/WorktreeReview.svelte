<script lang="ts">
  import { GitBranch, RefreshCw } from '@lucide/svelte';
  import Badge from './Badge.svelte';
  import Button from './Button.svelte';
  import Card from './Card.svelte';
  import Dialog from './Dialog.svelte';
  import ElapsedTime from './ElapsedTime.svelte';
  import InlineNotice from './InlineNotice.svelte';
  import LoadingActionContent from './LoadingActionContent.svelte';
  import { worktreeStore as defaultStore, type WorktreeStore } from '../stores/worktrees.svelte';
  import { formatBytes, formatTimeAgo } from '../utils/format';
  import { worktreeAgentLabel, worktreeStateDetail, worktreeStateLabel } from '../utils/worktrees';

  /**
   * Finished Git worktrees (#505): review and remove checkouts whose work is
   * committed. Removal goes through Git and keeps branches; the backend
   * re-checks every fact right before it runs.
   */
  let { store = defaultStore }: { store?: WorktreeStore } = $props();

  let idleDays = $derived(store.inventory?.idle_days ?? 14);
  let busy = $derived(store.phase === 'scanning' || store.phase === 'removing');
</script>

<section class="space-y-3" aria-labelledby="worktree-review-title">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="min-w-0">
      <h2 id="worktree-review-title" class="flex items-center gap-2 text-body font-semibold">
        <GitBranch size={15} class="text-muted-foreground" />
        Git worktrees
      </h2>
      <p class="mt-0.5 text-meta text-muted-foreground">
        Checkouts that agents and you created per task. A worktree is finished when everything in it is committed and on a
        branch, nothing uses it, and nobody has worked in it for {idleDays} days.
      </p>
    </div>
    <Button variant="outline" size="sm" onclick={() => void store.scan()} disabled={busy}>
      <LoadingActionContent busy={store.phase === 'scanning'} busyLabel="Checking…">
        <RefreshCw size={13} />
        {store.inventory ? 'Check again' : 'Check worktrees'}
      </LoadingActionContent>
    </Button>
  </div>

  {#if store.error}
    <InlineNotice variant="destructive" title="Worktrees" message={store.error} />
  {/if}

  {#if store.result && (store.result.removed.length > 0 || store.result.failed.length > 0)}
    <InlineNotice
      variant={store.result.failed.length > 0 ? 'warning' : 'success'}
      title={`Removed ${store.result.removed.length} ${store.result.removed.length === 1 ? 'worktree' : 'worktrees'} · ${formatBytes(store.result.reclaimed_bytes)}`}
      message={store.result.failed.length > 0
        ? store.result.failed.map((failure) => `${failure.name}: ${failure.message}`).join(' ')
        : 'Their branches and commits are still in the repository.'}
    />
  {/if}

  {#if store.forgotten !== null && store.forgotten > 0}
    <InlineNotice
      variant="success"
      title={`Git forgot ${store.forgotten} gone ${store.forgotten === 1 ? 'worktree' : 'worktrees'}`}
      message="Only Git's own records changed. Their branches are still there."
    />
  {/if}

  {#if store.phase === 'scanning' && store.startedAt}
    <Card class="p-4 text-meta text-muted-foreground">
      <span aria-live="polite">Checking the repositories in your project folders…</span>
      <ElapsedTime since={store.startedAt} />
    </Card>
  {:else if store.inventory}
    {#if store.inventory.repositories === 0}
      <Card class="p-4 text-meta text-muted-foreground">
        No repository found in your project folders. Add the folder that holds your code above.
      </Card>
    {:else if store.items.length === 0}
      <Card class="p-4 text-meta text-muted-foreground">
        No extra worktrees in {store.inventory.repositories}
        {store.inventory.repositories === 1 ? 'repository' : 'repositories'}.
      </Card>
    {:else}
      <div class="overflow-hidden rounded-xl border border-border bg-card divide-y divide-border">
        {#each store.items as item (item.id)}
          {@const removable = item.state === 'finished'}
          <article class="flex items-start gap-3 px-4 py-3" data-worktree-state={item.state}>
            <input
              type="checkbox"
              class="mt-1 accent-control"
              checked={store.selectedIds.includes(item.id)}
              disabled={!removable || busy}
              onchange={() => store.toggle(item.id)}
              aria-label={`Select worktree ${item.name}`}
            />
            <div class="min-w-0 flex-1 space-y-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-meta font-semibold">{item.name}</span>
                <span class="text-caption text-muted-foreground">{item.branch ?? `detached at ${item.head}`}</span>
                {#if worktreeAgentLabel(item.agent)}
                  <Badge variant="ai">{worktreeAgentLabel(item.agent)}</Badge>
                {/if}
                <span
                  class={`rounded border px-1.5 py-0.5 text-caption ${removable ? 'border-primary/40 bg-primary/10 text-foreground' : 'border-border bg-secondary/60 text-muted-foreground'}`}
                  title={worktreeStateDetail(item, idleDays)}
                >{worktreeStateLabel(item.state)}</span>
              </div>
              <div class="truncate font-mono text-caption text-muted-foreground" title={item.path}>{item.path}</div>
              <p class="text-caption text-muted-foreground">
                {#if item.last_activity}{`Last worked on ${formatTimeAgo(item.last_activity)} · `}{/if}{worktreeStateDetail(item, idleDays)}
              </p>
            </div>
            <div class="min-w-16 shrink-0 pt-0.5 text-right">
              <span class="block text-body font-semibold tabular-nums">{formatBytes(item.allocated_bytes)}</span>
              {#if !item.measurement_complete && item.state !== 'missing'}
                <span class="text-caption text-muted-foreground">at least</span>
              {/if}
            </div>
          </article>
        {/each}
      </div>

      {#if store.missing.length > 0}
        <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-secondary/50 px-3 py-2 text-meta">
          <p class="text-muted-foreground">
            {store.missing.length} {store.missing.length === 1 ? 'worktree record points' : 'worktree records point'} to a folder that is gone.
            Git can forget them; nothing on disk changes and branches stay.
          </p>
          <Button variant="outline" size="sm" disabled={busy} onclick={() => void store.forgetMissing()}>
            Forget {store.missing.length} gone
          </Button>
        </div>
      {/if}

      <div class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-caption text-muted-foreground">
          {store.finished.length} finished ·
          {store.inventory.rule_enabled
            ? 'one-button Clean includes them.'
            : 'turn on the worktree rule in Settings to include them in Clean.'}
        </p>
        <Button
          variant="destructive"
          size="sm"
          disabled={store.selectedItems.length === 0 || busy || store.phase === 'confirming'}
          onclick={() => void store.review()}
        >
          Remove {store.selectedItems.length || ''} · {formatBytes(store.selectedBytes)}
        </Button>
      </div>
    {/if}
  {/if}
</section>

{#if store.phase === 'confirming' || store.phase === 'removing'}
  <Dialog onCancel={() => store.cancelReview()} size="md" tone="destructive" aria-labelledby="worktree-confirm-title">
    <div class="space-y-4 p-5">
      <h2 id="worktree-confirm-title" class="text-body font-semibold">
        Remove {store.preview?.items.length ?? store.selectedItems.length} finished
        {(store.preview?.items.length ?? 0) === 1 ? 'worktree' : 'worktrees'}?
      </h2>
      <p class="text-meta text-muted-foreground">
        Git removes each checkout folder, including its ignored build output and dependencies. This is permanent.
        Branches and commits stay in the repository, so you can check a branch out again later. neati checks each one
        again right before removal and skips any that changed.
      </p>
      {#if store.preview}
        <ul class="max-h-48 space-y-1 overflow-auto text-meta">
          {#each store.preview.items as item (item.id)}
            <li class="flex justify-between gap-3">
              <span class="truncate">{item.name} <span class="text-muted-foreground">· {item.branch ?? `detached at ${item.head}`}</span></span>
              <span class="shrink-0 whitespace-nowrap tabular-nums">{formatBytes(item.allocated_bytes)}</span>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" onclick={() => store.cancelReview()} disabled={store.phase === 'removing'}>Cancel</Button>
        <Button variant="destructive" size="sm" onclick={() => void store.remove()} disabled={store.phase === 'removing'}>
          <LoadingActionContent busy={store.phase === 'removing'} busyLabel="Removing…">
            Remove permanently · {formatBytes(store.preview?.total_bytes ?? 0)}
          </LoadingActionContent>
        </Button>
      </div>
    </div>
  </Dialog>
{/if}
