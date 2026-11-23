<script lang="ts">
  import Card from './Card.svelte';
  import ElapsedTime from './ElapsedTime.svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import { cleanPhaseLabel, type CleanProgress } from '../utils/cleanProgress';

  /**
   * A running cleanup: the target being removed, its place in the plan and the
   * elapsed time (#491), e.g. `Removing npm cache · 3 of 12 · 8 s`. The bar
   * counts finished targets only; nothing here estimates time remaining.
   */
  let { progress, startedAt, compact = false }: {
    progress: CleanProgress;
    startedAt: number | null;
    compact?: boolean;
  } = $props();
</script>

<Card class="{compact ? 'p-3.5' : 'p-4'} bg-secondary/60 border-primary/40 transition-colors duration-normal">
  <div class={compact ? 'space-y-1.5' : 'space-y-2'} aria-busy="true" data-clean-progress>
    <div class="flex items-center gap-2 text-meta font-medium text-foreground">
      <LoadingIndicator word="cleaning" size="xs" />
      <p class="min-w-0 break-words">
        <span role="status" aria-live="polite">{cleanPhaseLabel(progress.currentItem, progress.index, progress.total)}</span>
        {#if startedAt !== null}<span class="text-muted-foreground"> · <ElapsedTime since={startedAt} /></span>{/if}
      </p>
    </div>
    <ProgressBar value={progress.percent} height="h-2" color="bg-primary" />
  </div>
</Card>
