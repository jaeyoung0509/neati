<script lang="ts">
  import { Clock, Moon } from '@lucide/svelte';
  import type { AwakeBehavior } from '../models/types';
  import { awakeStore } from '../stores/awake.svelte';
  import { awaitingFirstAnswer } from '../utils/loadPhase';
  import Button from './Button.svelte';
  import LoadingActionContent from './LoadingActionContent.svelte';

  /**
   * Manual Keep Awake durations. One request at a time: every duration is
   * disabled while a Keep Awake change saves or before the current state has
   * been read, and the chosen one says it is starting (#491).
   */
  const DURATIONS: readonly { minutes: number | null; label: string }[] = [
    { minutes: 30, label: '30 Minutes' },
    { minutes: 60, label: '1 Hour' },
    { minutes: 120, label: '2 Hours' },
    { minutes: null, label: 'Indefinite' },
  ];

  // Manual timer behavior is intentionally separate from automatic rules.
  let manualBehavior = $state<AwakeBehavior>('prevent_system_sleep');
  let locked = $derived(awakeStore.isLoading || awaitingFirstAnswer(awakeStore.loadPhase));

  function isStarting(minutes: number | null): boolean {
    const pending = awakeStore.pending;
    return pending?.kind === 'manual' && pending.durationSecs === (minutes === null ? null : minutes * 60);
  }

  function start(minutes: number | null) {
    // A rejected start is already recorded in the store's error.
    void awakeStore.setManual(minutes === null ? null : minutes * 60, manualBehavior).catch(() => undefined);
  }
</script>

<section class="space-y-3" aria-labelledby="manual-heading">
  <div class="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
    <div>
      <h2 id="manual-heading" class="text-sm font-semibold tracking-tight text-foreground">Quick Manual Duration</h2>
      <p class="text-meta text-muted-foreground mt-0.5">Temporarily keep the computer awake regardless of automatic rules.</p>
    </div>
    <div class="flex items-center gap-1.5 p-1 rounded-lg bg-card/80 border border-border/70 text-meta" role="group" aria-label="Manual sleep behavior">
      <button
        type="button"
        aria-pressed={manualBehavior === 'prevent_system_sleep'}
        onclick={() => (manualBehavior = 'prevent_system_sleep')}
        class="px-2.5 py-1 rounded text-meta font-medium transition-colors {manualBehavior === 'prevent_system_sleep' ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:text-foreground'}"
        title="Work continues while you are away; the display may turn off"
      >
        Computer awake
      </button>
      <button
        type="button"
        aria-pressed={manualBehavior === 'keep_display_awake'}
        onclick={() => (manualBehavior = 'keep_display_awake')}
        class="px-2.5 py-1 rounded text-meta font-medium transition-colors {manualBehavior === 'keep_display_awake' ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:text-foreground'}"
        title="Keeps both the computer and display awake while idle"
      >
        Computer + display
      </button>
    </div>
  </div>

  <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
    {#each DURATIONS as duration (duration.label)}
      {@const starting = isStarting(duration.minutes)}
      <Button variant="outline" size="md" onclick={() => start(duration.minutes)} disabled={locked} aria-busy={starting} class="flex-col h-auto py-3 text-meta gap-1">
        {#if duration.minutes === null}
          <Moon size={16} class="text-warning" aria-hidden="true" />
        {:else}
          <Clock size={16} class="text-muted-foreground" aria-hidden="true" />
        {/if}
        <span class="font-medium text-foreground">
          <LoadingActionContent busy={starting} busyLabel="Starting…">{duration.label}</LoadingActionContent>
        </span>
      </Button>
    {/each}
  </div>
</section>
