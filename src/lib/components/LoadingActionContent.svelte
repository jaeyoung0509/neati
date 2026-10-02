<script lang="ts">
  import type { ComponentProps, Snippet } from 'svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';

  type IndicatorProps = ComponentProps<typeof LoadingIndicator>;

  let { busy, busyLabel, word = 'loading', tone = 'ink', motion = 'write', size = 'xs', children }: {
    busy: boolean;
    busyLabel: string;
    word?: IndicatorProps['word'];
    tone?: IndicatorProps['tone'];
    motion?: IndicatorProps['motion'];
    size?: IndicatorProps['size'];
    children: Snippet;
  } = $props();
</script>

<!-- Both slots contribute their width; only the active operation is exposed. -->
<span class="inline-grid items-center">
  <span class="col-start-1 row-start-1 inline-flex items-center justify-center gap-1.5" class:invisible={busy} aria-hidden={busy}>
    {@render children()}
  </span>
  <span class="col-start-1 row-start-1 inline-flex items-center justify-center gap-1.5" class:invisible={!busy} aria-hidden={!busy}>
    <LoadingIndicator {word} {tone} {motion} {size} active={busy} />
    {#if busyLabel}<span>{busyLabel}</span>{/if}
  </span>
</span>
