<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';

  type IndicatorProps = ComponentProps<typeof LoadingIndicator>;

  /**
   * The waiting counterpart of EmptyState: shown while a first answer is
   * pending, so an empty result never reads as final before it is (#491).
   */
  let { message, detail = '', word = 'loading', class: className = '' }: {
    message: string;
    detail?: string;
    word?: IndicatorProps['word'];
    class?: string;
  } = $props();
</script>

<div class="flex flex-col items-center justify-center gap-3 rounded-xl border border-border/70 bg-card/60 px-6 py-10 text-center {className}" role="status" aria-live="polite">
  <LoadingIndicator size="md" {word} class="text-primary" />
  <p class="text-body font-medium text-foreground">{message}</p>
  {#if detail}<p class="text-meta text-muted-foreground">{detail}</p>{/if}
</div>
