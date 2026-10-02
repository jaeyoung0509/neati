<script lang="ts">
  import { onMount } from 'svelte';
  import { Square } from '@lucide/svelte';
  import { scanStore } from '../stores/scan.svelte';
  import Button from './Button.svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';
  import AnimatedValue from './AnimatedValue.svelte';
  import RippleScene from './RippleScene.svelte';

  let clock = $state(Date.now());
  let elapsed = $derived(Math.max(0, Math.floor((clock - (scanStore.scanStartedAt ?? clock)) / 1000)));
  let elapsedLabel = $derived(elapsed < 60 ? `${elapsed}s elapsed` : `${Math.floor(elapsed / 60)}m ${elapsed % 60}s elapsed`);
  onMount(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    const syncClock = () => {
      if (timer !== undefined) clearInterval(timer);
      timer = undefined;
      if (!document.hidden) {
        clock = Date.now();
        timer = setInterval(() => { clock = Date.now(); }, 1000);
      }
    };
    syncClock();
    document.addEventListener('visibilitychange', syncClock);
    return () => { if (timer !== undefined) clearInterval(timer); document.removeEventListener('visibilitychange', syncClock); };
  });
</script>

<section class="scan-progress" aria-label="Storage scan progress" aria-busy="true">
  <div class="scan-ripples"><RippleScene activity={scanStore.isCancelling ? 'stopping' : 'working'} /></div>
  <div class="relative flex flex-wrap items-start justify-between gap-3">
    <div class="flex min-w-0 flex-1 items-start gap-3">
      <span class="shrink-0 text-primary" aria-hidden="true"><LoadingIndicator size="md" word={scanStore.isCancelling ? 'working' : 'scanning'} active={!scanStore.isCancelling} /></span>
      <div class="min-w-0">
        <p class="text-sm font-semibold" role="status">
          {scanStore.isCancelling ? 'Stopping scan' : scanStore.isRefreshingAfterClean ? 'Checking storage after cleanup' : 'Checking storage'}
        </p>
        <p class="mt-1 max-w-prose text-body text-muted-foreground">
          {scanStore.isCancelling ? 'Keeping the locations checked so far.' : scanStore.isRefreshingAfterClean
            ? 'Cleanup finished. A new scan is checking what remains before results appear.'
            : 'Scanning application and development caches.'}
        </p>
      </div>
    </div>
    <Button variant="outline" size="sm" motion="paint" disabled={scanStore.isCancelling || !scanStore.scanId} title={!scanStore.scanId ? 'Waiting for the scan to start.' : undefined} onclick={() => void scanStore.cancelScan()} ariaLabel={scanStore.isCancelling ? 'Stopping scan' : 'Stop scan'} class="shrink-0 gap-1.5">
      <Square size={12} aria-hidden="true" /><span>{scanStore.isCancelling ? 'Stopping…' : 'Stop scan'}</span>
    </Button>
  </div>
  <div class="scan-facts relative mt-5 grid gap-4">
    <div class="min-w-0">
      <p class="text-meta text-muted-foreground">Current area</p>
      <p class="mt-1 truncate text-body font-medium" title={scanStore.currentRoot?.name}>{scanStore.currentRoot?.name ?? (scanStore.scanId ? 'Checking scan locations…' : 'Preparing scan…')}</p>
    </div>
    <div>
      <p class="text-meta text-muted-foreground">Found so far</p>
      <p class="mt-1 font-mono text-body"><AnimatedValue value={String(scanStore.foundItemCount)} active={!scanStore.isCancelling} /> {scanStore.foundItemCount === 1 ? 'item' : 'items'}</p>
    </div>
    <p class="text-meta font-mono tabular-nums text-muted-foreground self-end">{elapsedLabel}</p>
  </div>
  {#if scanStore.currentRoot}
    <details class="relative mt-4 text-caption text-muted-foreground">
      <summary class="w-fit cursor-pointer rounded-sm underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">Show current path</summary>
      <code class="mt-2 block break-all rounded-lg bg-secondary p-2 font-mono">{scanStore.currentRoot.path}</code>
    </details>
  {/if}
</section>
<section class="scan-results-placeholder rounded-xl border border-border bg-card p-4" aria-label="Results being checked">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h2 class="text-body font-semibold">Your cleanup results</h2>
    <span class="text-meta text-muted-foreground">Available after this scan</span>
  </div>
  <div class="mt-3 space-y-3" aria-hidden="true">
    {#each [0, 1, 2] as index}
      <div class="flex h-10 items-center gap-3">
        <span class="h-8 w-8 rounded-lg bg-secondary"></span>
        <span class="flex-1 space-y-2"><span class="block h-2 w-28 rounded bg-secondary"></span><span class="block h-2 rounded bg-secondary" style:width={`${38 + index * 10}%`}></span></span>
        <span class="h-2 w-16 rounded bg-secondary"></span>
      </div>
    {/each}
  </div>
</section>

<style>
  .scan-progress { position: relative; isolation: isolate; overflow: hidden; min-height: 188px; padding: 20px; border: 1px solid hsl(var(--border)); border-radius: 16px; background: hsl(var(--card)); }
  .scan-ripples { position: absolute; width: 390px; right: -48px; bottom: -82px; opacity: 0.52; z-index: -1; }
  .scan-facts { grid-template-columns: minmax(0, 1fr) auto auto; border-top: 1px solid hsl(var(--border)); padding-top: 16px; }
  @container (max-width: 460px) { .scan-facts { grid-template-columns: minmax(0, 1fr) auto; } .scan-facts > :last-child { grid-column: 1 / -1; } .scan-ripples { display: none; } }
</style>
