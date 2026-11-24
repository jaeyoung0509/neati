<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'default' | 'secondary' | 'outline' | 'success' | 'warning' | 'destructive' | 'ai';
    class?: string;
    children?: Snippet;
  }

  let {
    variant = 'default',
    class: className = '',
    children,
  }: Props = $props();

  const variantStyles = {
    default: 'bg-primary text-primary-foreground',
    secondary: 'bg-secondary text-secondary-foreground',
    outline: 'border border-border-strong text-foreground',
    success: 'status-chip-success text-success border',
    warning: 'status-chip-warning text-warning border',
    destructive: 'status-chip-destructive text-destructive border',
    ai: 'status-chip-ai text-ai border',
  };
</script>

<span
  class="inline-flex shrink-0 items-center whitespace-nowrap px-2 py-0.5 rounded-full text-caption font-medium tracking-tight {variantStyles[
    variant
  ]} {className}"
>
  {#if children}
    {@render children()}
  {/if}
</span>
