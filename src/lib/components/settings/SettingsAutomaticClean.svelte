<script lang="ts">
  import { onMount } from 'svelte';
  import type { AutomaticCleanInterval, AutomaticCleanStatus } from '../../models/types';
  import { settingsStore } from '../../stores/settings.svelte';
  import { tauriGetAutomaticCleanStatus } from '../../utils/tauri';
  import { automaticCleanSummary } from '../../utils/automaticClean';
  import Button from '../Button.svelte';
  import Card from '../Card.svelte';
  import Dialog from '../Dialog.svelte';
  import Switch from '../Switch.svelte';

  /**
   * Automatic Clean (#507): the one-button set on a schedule, off by default.
   * Turning it on asks once, because removal then happens without a click.
   */
  let { initialStatus = null }: { initialStatus?: AutomaticCleanStatus | null } = $props();

  let status = $state<AutomaticCleanStatus | null>(null);
  let confirming = $state(false);
  // The switch is a native checkbox: a click flips it before the person
  // confirms. Re-creating it after a cancel shows the saved state again.
  let switchKey = $state(0);

  function cancel() {
    confirming = false;
    switchKey += 1;
  }

  let settings = $derived({
    enabled: settingsStore.settings.automatic_clean?.enabled ?? false,
    interval: settingsStore.settings.automatic_clean?.interval ?? 'weekly',
  });

  async function refresh() {
    try {
      status = await tauriGetAutomaticCleanStatus();
    } catch {
      status = null;
    }
  }

  onMount(() => {
    status = initialStatus;
    if (!initialStatus) void refresh();
  });

  async function save(enabled: boolean, interval: AutomaticCleanInterval) {
    if (await settingsStore.save({ automatic_clean: { enabled, interval } })) {
      await refresh();
    }
  }

  function toggle(enabled: boolean) {
    if (enabled) {
      confirming = true;
    } else {
      void save(false, settings.interval);
    }
  }
</script>

<div class="space-y-3">
  <div>
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">Automatic Clean</h3>
    <p class="text-meta text-muted-foreground mt-1">
      Runs one-button Clean for you, with your cleanup rules, while neati is in the menu bar.
    </p>
  </div>
  <Card class="p-4 space-y-3 bg-card">
    <div class="flex items-start justify-between gap-5 text-meta">
      <div class="min-w-0">
        <div class="font-medium text-foreground">Clean automatically</div>
        <div class="text-meta text-muted-foreground mt-0.5">
          Only on AC power, after 10 minutes without keyboard or mouse use, and at most once per interval. Removal is
          permanent; anything in use is skipped.
        </div>
        {#if settings.enabled}
          <div class="mt-2 flex items-center gap-2">
            <label for="automatic-clean-interval" class="text-muted-foreground">How often</label>
            <select
              id="automatic-clean-interval"
              value={settings.interval}
              onchange={(event) =>
                void save(true, (event.target as HTMLSelectElement).value as AutomaticCleanInterval)}
              class="h-8 rounded-md border border-border-strong bg-card px-2 text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <option value="daily">Daily</option>
              <option value="weekly">Weekly</option>
            </select>
          </div>
        {/if}
      </div>
      {#key switchKey}
        <Switch
          checked={settings.enabled}
          disabled={status?.available === false}
          onchange={toggle}
          ariaLabel="Clean automatically"
        />
      {/key}
    </div>
    {#if status}
      <p class="text-caption text-muted-foreground" data-automatic-clean-summary>{automaticCleanSummary(status)}</p>
    {/if}
  </Card>
</div>

{#if confirming}
  <Dialog onCancel={cancel} size="md" tone="destructive" aria-labelledby="automatic-clean-confirm">
    <div class="space-y-4 p-5">
      <h2 id="automatic-clean-confirm" class="text-body font-semibold">Clean automatically?</h2>
      <p class="text-meta text-muted-foreground">
        neati will permanently remove the one-button Clean set without asking each time: verified caches and what your
        project cleanup rules include. It runs {settings.interval === 'daily' ? 'once a day' : 'once a week'}, only on AC
        power while you are away from the Mac, and tells you what it removed.
      </p>
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" onclick={cancel}>Cancel</Button>
        <Button
          variant="destructive"
          size="sm"
          onclick={() => {
            confirming = false;
            void save(true, settings.interval);
          }}
        >
          Turn on
        </Button>
      </div>
    </div>
  </Dialog>
{/if}
