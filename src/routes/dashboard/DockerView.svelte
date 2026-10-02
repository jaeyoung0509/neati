<script lang="ts">
  import { onMount } from 'svelte';
  import { dockerStore } from '../../lib/stores/docker.svelte';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import { formatBytes } from '../../lib/utils/format';
  import { withMinimumDuration } from '../../lib/utils/async';
  import { modalDialog } from '../../lib/utils/modalDialog';
  import Button from '../../lib/components/Button.svelte';
  import Card from '../../lib/components/Card.svelte';
  import Badge from '../../lib/components/Badge.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import EmptyState from '../../lib/components/EmptyState.svelte';
  import {
    Container,
    RotateCw,
    Trash2,
    Layers,
    Server,
    HardDrive,
    AlertCircle,
  } from '@lucide/svelte';

  let status = $derived(dockerStore.status);
  let overview = $derived(status?.overview);
  let confirmVolumePrune = $state(false);

  onMount(() => {
    void platformContextStore.load();
    void dockerStore.refresh();
  });

  async function pruneVolumes() {
    confirmVolumePrune = false;
    await dockerStore.pruneTarget('container.docker.unused_volumes');
  }

  let danglingImages = $derived(status?.images?.filter((i) => i.is_dangling) ?? []);
  let danglingBytes = $derived(danglingImages.reduce((sum, i) => sum + i.size_bytes, 0));

  let isRefreshing = $state(false);

  async function handleRefresh() {
    if (isRefreshing) return;
    isRefreshing = true;
    try {
      await withMinimumDuration(dockerStore.refresh(), 600);
    } finally {
      isRefreshing = false;
    }
  }
</script>

<div class="space-y-6">
  <!-- Page Header -->
  <PageHeader
    title="Containers"
    subtitle={status?.version || 'Inspect and safely prune Docker containers, build cache, and dangling images.'}
    icon={Container}
  >
    {#snippet badge()}
      {#if status?.is_running}
        <Badge variant="success">Daemon Running</Badge>
      {:else if status?.is_available}
        <Badge variant="warning">Daemon Stopped</Badge>
      {:else}
        <Badge variant="secondary">Not Installed</Badge>
      {/if}
    {/snippet}

    {#snippet actions()}
      <Button
        variant="outline"
        size="sm"
        disabled={isRefreshing || dockerStore.isLoading || dockerStore.isPruning}
        onclick={handleRefresh}
        class="gap-1.5 text-xs"
      >
        <LoadingActionContent busy={isRefreshing || dockerStore.isLoading} busyLabel="Refreshing…">
          <RotateCw size={13} aria-hidden="true" /><span>Refresh</span>
        </LoadingActionContent>
      </Button>
    {/snippet}
  </PageHeader>

  {#if dockerStore.error}
    <InlineNotice
      variant="destructive"
      title="Docker Error"
      message={dockerStore.error}
      onDismiss={() => (dockerStore.error = null)}
    />
  {/if}

  {#if !status?.is_running}
    <EmptyState
      icon={Container}
      title="Docker Daemon is Inactive"
      description={platformContextStore.containerRuntimeHint
        ? `Start ${platformContextStore.containerRuntimeHint} to inspect images, containers, and build cache storage.`
        : 'Start a container runtime to inspect images, containers, and build cache storage.'}
    />
  {:else if overview}
    <!-- Storage Breakdown Grid -->
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5 gap-3">
      <!-- Build Cache (Safe) -->
      <Card class="p-4 space-y-3 bg-card/60 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Layers size={15} class="text-primary" />
              <span>Build Cache</span>
            </div>
            <Badge variant="success">Safe</Badge>
          </div>
          <div class="mt-3">
            <div class="text-xl font-bold font-mono text-foreground">
              {formatBytes(overview.build_cache.reclaimable_bytes)}
            </div>
            <p class="text-meta text-muted-foreground mt-0.5">Unused BuildKit layers</p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={dockerStore.isPruning || overview.build_cache.reclaimable_bytes === 0}
          onclick={() => dockerStore.pruneTarget('container.docker.builder')}
          class="w-full text-xs gap-1.5 min-h-[30px]"
        >
          <LoadingActionContent busy={dockerStore.isPruning} busyLabel="Pruning…" word="cleaning">
            <Trash2 size={12} aria-hidden="true" /><span>Prune Cache</span>
          </LoadingActionContent>
        </Button>
      </Card>

      <!-- Dangling Images (Safe) -->
      <Card class="p-4 space-y-3 bg-card/60 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Container size={15} class="text-success" />
              <span>Dangling</span>
            </div>
            <Badge variant="success">Safe</Badge>
          </div>
          <div class="mt-3">
            <div class="text-xl font-bold font-mono text-foreground">
              {danglingBytes > 0 ? formatBytes(danglingBytes) : (danglingImages.length > 0 ? `${danglingImages.length} images` : '0 B')}
            </div>
            <p class="text-meta text-muted-foreground mt-0.5">Untagged layers</p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={dockerStore.isPruning || danglingImages.length === 0}
          onclick={() => dockerStore.pruneTarget('container.docker.dangling_images')}
          class="w-full text-xs gap-1.5 min-h-[30px]"
        >
          <LoadingActionContent busy={dockerStore.isPruning} busyLabel="Pruning…" word="cleaning">
            <Trash2 size={12} aria-hidden="true" /><span>Prune Dangling</span>
          </LoadingActionContent>
        </Button>
      </Card>

      <!-- Unused Images (Rebuild) -->
      <Card class="p-4 space-y-3 bg-card/60 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Container size={15} class="text-warning" />
              <span>Unused Images</span>
            </div>
            <Badge variant="warning">Rebuild</Badge>
          </div>
          <div class="mt-3">
            <div class="text-xl font-bold font-mono text-foreground">
              {formatBytes(overview.images.reclaimable_bytes)}
            </div>
            <p class="text-meta text-muted-foreground mt-0.5">Unreferenced images</p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={dockerStore.isPruning || overview.images.reclaimable_bytes === 0}
          onclick={() => dockerStore.pruneTarget('container.docker.unused_images')}
          class="w-full text-xs gap-1.5 min-h-[30px]"
        >
          <LoadingActionContent busy={dockerStore.isPruning} busyLabel="Pruning…" word="cleaning">
            <Trash2 size={12} aria-hidden="true" /><span>Remove Unused</span>
          </LoadingActionContent>
        </Button>
      </Card>

      <!-- Stopped Containers (Rebuild) -->
      <Card class="p-4 space-y-3 bg-card/60 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Server size={15} class="text-warning" />
              <span>Containers</span>
            </div>
            <Badge variant="warning">Rebuild</Badge>
          </div>
          <div class="mt-3">
            <div class="text-xl font-bold font-mono text-foreground">
              {formatBytes(overview.containers.reclaimable_bytes)}
            </div>
            <p class="text-meta text-muted-foreground mt-0.5">Exited container data</p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={dockerStore.isPruning || overview.containers.reclaimable_bytes === 0}
          onclick={() => dockerStore.pruneTarget('container.docker.stopped_containers')}
          class="w-full text-xs gap-1.5 min-h-[30px]"
        >
          <LoadingActionContent busy={dockerStore.isPruning} busyLabel="Pruning…" word="cleaning">
            <Trash2 size={12} aria-hidden="true" /><span>Prune Containers</span>
          </LoadingActionContent>
        </Button>
      </Card>

      <!-- Unused Volumes (Manual) -->
      <Card class="p-4 space-y-3 bg-card/60 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <HardDrive size={15} class="text-destructive" />
              <span>Volumes</span>
            </div>
            <Badge variant="danger">Manual</Badge>
          </div>
          <div class="mt-3">
            <div class="text-xl font-bold font-mono text-foreground">
              {formatBytes(overview.volumes.reclaimable_bytes)}
            </div>
            <p class="text-meta text-muted-foreground mt-0.5">of {formatBytes(overview.volumes.total_bytes)} total</p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={dockerStore.isPruning || overview.volumes.reclaimable_bytes === 0}
          onclick={() => (confirmVolumePrune = true)}
          class="w-full text-xs gap-1.5 text-destructive hover:text-destructive min-h-[30px]"
        >
          <LoadingActionContent busy={dockerStore.isPruning} busyLabel="Pruning…" word="cleaning">
            <Trash2 size={12} aria-hidden="true" /><span>Prune Volumes</span>
          </LoadingActionContent>
        </Button>
      </Card>
    </div>

    <!-- Active Containers & Images Table -->
    {#if status?.containers && status.containers.length > 0}
      <div class="space-y-3 pt-2">
        <h3 class="text-xs font-semibold text-muted-foreground tracking-normal">
          Detected Containers ({status.containers.length})
        </h3>
        <div class="space-y-2 max-h-60 overflow-y-auto scroll-stable">
          {#each status.containers as container}
            <div class="flex items-center justify-between p-3 rounded-lg bg-card/70 border border-border/60 text-xs">
              <div class="space-y-0.5">
                <div class="flex items-center gap-2 font-medium text-foreground">
                  <span>{container.name}</span>
                  <span class="text-muted-foreground font-mono text-caption">({container.image})</span>
                </div>
                <div class="flex items-center gap-2 text-meta text-muted-foreground">
                  <span class={container.is_running ? 'text-success font-medium' : 'text-muted-foreground'}>
                    ● {container.state}
                  </span>
                </div>
              </div>
              <span class="font-mono text-muted-foreground">
                {formatBytes(container.size_bytes)}
              </span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  {/if}

  {#if confirmVolumePrune}
    <dialog
      use:modalDialog={{ onCancel: () => (confirmVolumePrune = false), initialFocus: '#volume-prune-cancel' }}
      aria-labelledby="volume-prune-title" aria-describedby="volume-prune-description"
      class="m-auto w-[calc(100%-2rem)] max-w-sm max-h-[calc(100%-2rem)] overflow-y-auto scroll-stable space-y-4 rounded-xl border border-border bg-card p-5 text-foreground shadow-2xl backdrop:bg-background/80 backdrop:backdrop-blur-sm [overflow-wrap:anywhere]"
    >
        <div class="flex items-start gap-3">
          <div class="mt-0.5 text-destructive"><AlertCircle size={20} /></div>
          <div>
            <h3 id="volume-prune-title" class="text-sm font-semibold">Prune unused Docker volumes?</h3>
            <p id="volume-prune-description" class="mt-1 text-xs leading-relaxed text-muted-foreground">
              Docker reports {formatBytes(overview?.volumes.reclaimable_bytes ?? 0)} as reclaimable. Volumes may contain persistent application data and cannot be restored by neati.
            </p>
          </div>
        </div>
        <div class="flex justify-end gap-2">
          <Button id="volume-prune-cancel" variant="ghost" size="sm" onclick={() => (confirmVolumePrune = false)}>Cancel</Button>
          <Button variant="destructive" size="sm" onclick={pruneVolumes}>Prune Volumes</Button>
        </div>
    </dialog>
  {/if}
</div>
