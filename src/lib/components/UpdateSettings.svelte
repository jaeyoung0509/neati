<script lang="ts">
  import { settingsStore } from '../stores/settings.svelte';
  import { updateStore } from '../stores/updates.svelte';
  import Button from './Button.svelte';
  import InlineNotice from './InlineNotice.svelte';
  import Switch from './Switch.svelte';
  import { updateStatusCopy } from '../utils/updateCopy';

  let { releasesUrl = null }: { releasesUrl?: string | null } = $props();
  let enabled = $derived(settingsStore.settings.check_updates_on_launch);
  let status = $derived(updateStatusCopy(updateStore.result));
  let available = $derived(updateStore.available);
  let busy = $derived(updateStore.phase !== 'idle');
</script>

<div class="space-y-3 text-meta" data-update-settings>
  <div class="flex items-center justify-between gap-3">
    <div>
      <div class="font-medium text-foreground">Check for updates when neati opens</div>
      <div class="text-muted-foreground">
        Off by default. When on, neati reads the signed release manifest on GitHub once per launch. Nothing is downloaded until you choose Install.
      </div>
    </div>
    <Switch
      checked={enabled}
      onchange={() => settingsStore.save({ check_updates_on_launch: !enabled })}
      ariaLabel="Check for updates when neati opens"
    />
  </div>

  <div class="flex flex-wrap items-center gap-2">
    <Button size="sm" variant="outline" disabled={busy} onclick={() => void updateStore.checkNow()}>
      {updateStore.phase === 'checking' ? 'Checking…' : 'Check now'}
    </Button>
    {#if status}<span role="status" class="text-muted-foreground">{status}</span>{/if}
  </div>

  {#if available}
    <div class="space-y-2 rounded-xl border border-border bg-secondary p-3">
      <div class="font-medium text-foreground">neati {available.version} is available</div>
      {#if available.notes}<p class="whitespace-pre-line text-muted-foreground">{available.notes}</p>{/if}
      <p class="text-muted-foreground">
        neati downloads the update, verifies its signature, replaces this copy and restarts. Save any open work in the Quick Panel first.
      </p>
      <Button size="sm" disabled={busy} onclick={() => void updateStore.installAvailable()}>
        {updateStore.phase === 'installing' ? 'Installing…' : 'Install and restart'}
      </Button>
    </div>
  {/if}

  {#if updateStore.error}
    <InlineNotice variant="destructive" title="Update not installed" message={updateStore.error} />
  {/if}

  {#if releasesUrl && (updateStore.result?.status === 'unavailable' || updateStore.result?.status === 'failed')}
    <a class="inline-block text-primary underline underline-offset-2" href={releasesUrl} target="_blank" rel="noreferrer noopener">
      Open the releases page
    </a>
  {/if}
</div>
