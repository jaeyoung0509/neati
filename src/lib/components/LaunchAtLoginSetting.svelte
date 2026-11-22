<script lang="ts">
  import { onMount } from 'svelte';
  import { launchAtLoginStore } from '../stores/launchAtLogin.svelte';
  import Switch from './Switch.svelte';

  onMount(() => { void launchAtLoginStore.load(); });
  let reason = $derived(launchAtLoginStore.unavailableReason);
</script>

<div class="space-y-1 text-meta" data-launch-at-login>
  <div class="flex items-center justify-between gap-3">
    <div>
      <div class="font-medium text-foreground">Start neati when you log in</div>
      <div class="text-muted-foreground">
        {#if reason}
          {reason}
        {:else}
          neati starts in the menu bar without opening a window. It takes effect the next time you log in.
        {/if}
      </div>
    </div>
    <Switch
      checked={launchAtLoginStore.enabled}
      disabled={launchAtLoginStore.busy || launchAtLoginStore.state === null || reason !== null}
      onchange={() => void launchAtLoginStore.set(!launchAtLoginStore.enabled)}
      ariaLabel="Start neati when you log in"
    />
  </div>
  {#if launchAtLoginStore.error}
    <p role="alert" class="text-destructive">{launchAtLoginStore.error}</p>
  {/if}
</div>
