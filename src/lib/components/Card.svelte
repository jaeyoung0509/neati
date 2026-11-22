<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../utils/cn';

  interface Props {
    /**
     * `card` is the white page surface, `subtle` the secondary grouped surface,
     * and `plain` a transparent wrapper for rows that already sit in one surface.
     */
    surface?: 'card' | 'subtle' | 'plain';
    class?: string;
    children?: Snippet;
  }

  let { surface = 'card', class: className = '', children }: Props = $props();

  const surfaceStyles = {
    card: 'bg-card text-card-foreground border border-border',
    subtle: 'bg-secondary text-secondary-foreground border border-border',
    plain: 'bg-transparent border border-transparent',
  };
</script>

<!-- A card is a static surface. Interactive content goes in a Button or a
     native link inside it, so every action stays keyboard reachable. -->
<div class={cn('rounded-xl p-4', surfaceStyles[surface], className)}>
  {#if children}
    {@render children()}
  {/if}
</div>
