<script lang="ts">
  import { onMount } from 'svelte';
  import { scanStore } from '../stores/scan.svelte';
  import { storageAccessStore } from '../stores/storageAccess.svelte';
  import Button from './Button.svelte';

  let { contextual = false }: { contextual?: boolean } = $props();
  let gapCount = $derived(scanStore.lastScan?.gaps
    ?.filter((gap) => gap.kind === 'full_disk_access')
    .reduce((total, gap) => total + gap.count, 0) ?? 0);
  let busy = $derived(scanStore.isScanning || scanStore.isCleaning);

  onMount(() => storageAccessStore.subscribe());
  $effect(() => {
    if (!busy) storageAccessStore.checkWhenIdle();
  });
</script>

<div class="space-y-3 text-meta leading-relaxed" data-storage-access-setup>
  <div>
    <h4 class="font-medium text-foreground">{contextual ? 'Check protected locations' : 'macOS storage access'}</h4>
    <p class="mt-1 text-muted-foreground">
      Full Disk Access lets neati inspect protected Mail, Messages and application containers.
      You choose whether to allow it. Scanning never deletes anything.
    </p>
  </div>
  <details class="rounded-lg border border-border p-3" open={contextual}>
    <summary class="cursor-pointer font-medium text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">How to allow access</summary>
    <ol class="mt-2 list-decimal space-y-1 pl-5 text-muted-foreground">
      <li>Open System Settings → Privacy &amp; Security → Full Disk Access.</li>
      <li>Click + and select the neati.app you use, usually in Applications, then enable it.</li>
      <li>Return to neati to scan again. If macOS asks you to quit and reopen the app, follow that prompt, then scan again.</li>
    </ol>
    <p class="mt-2 text-muted-foreground">
      This permission does not grant administrator access or make every file removable.
      After replacing or updating the app, check access again if locations become unreadable.
    </p>
  </details>
  <p class="text-muted-foreground" role="status" aria-live="polite">
    {#if storageAccessStore.phase === 'opening'}
      Opening System Settings…
    {:else if storageAccessStore.phase === 'waiting'}
      Settings opened. Access will be checked with a fresh scan when you return. You can also choose Check Access.
    {:else if storageAccessStore.phase === 'queued'}
      Access will be checked after the current storage operation finishes.
    {:else if storageAccessStore.phase === 'checking'}
      Scanning to check actual access…
    {:else if scanStore.isScanning}
      Checking storage…
    {:else if gapCount > 0}
      The last scan could not read {gapCount} protected {gapCount === 1 ? 'location' : 'locations'}. Privacy or file permissions may be responsible. Unknown bytes stay outside totals.
    {:else if storageAccessStore.phase === 'checked'}
      Scan complete. No privacy-access gaps were reported. Other unavailable or protected locations may remain.
    {:else}
      A fresh scan checks each location. Opening Settings alone does not confirm access.
    {/if}
  </p>
  {#if storageAccessStore.error}
    <p class="text-destructive" role="alert">{storageAccessStore.error}</p>
  {/if}
  <div class="flex flex-wrap gap-2">
    <Button size="sm" variant="secondary" disabled={storageAccessStore.phase === 'opening' || storageAccessStore.phase === 'checking'} onclick={() => void storageAccessStore.openSettings()}>
      Open System Settings
    </Button>
    <Button size="sm" variant="outline" disabled={busy || storageAccessStore.phase === 'opening' || storageAccessStore.phase === 'checking'} onclick={() => void storageAccessStore.check()}>
      {storageAccessStore.phase === 'checking' ? 'Checking…' : 'Check Access'}
    </Button>
  </div>
</div>
