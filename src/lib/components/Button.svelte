<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { cn } from '../utils/cn';

  /**
   * Every native button attribute (`type`, `aria-*`, `title`, `id`, handlers)
   * is forwarded. `class` is merged so a caller's override wins.
   */
  interface Props extends Omit<HTMLButtonAttributes, 'class' | 'children'> {
    variant?: 'primary' | 'secondary' | 'outline' | 'destructive' | 'ghost';
    size?: 'xs' | 'sm' | 'md' | 'icon';
    motion?: 'scale' | 'paint';
    class?: string;
    children?: Snippet;
  }

  let {
    variant = 'primary',
    size = 'md',
    motion = 'scale',
    type = 'button',
    class: className = '',
    children,
    ...rest
  }: Props = $props();

  const variantStyles = {
    primary:
      'neati-action text-action-foreground',
    secondary:
      'bg-secondary text-secondary-foreground hover:bg-secondary/80',
    outline:
      'border border-border-strong bg-transparent hover:bg-accent hover:text-accent-foreground',
    destructive:
      'bg-destructive text-destructive-foreground hover:bg-destructive/90',
    ghost:
      'hover:bg-accent hover:text-accent-foreground',
  };

  const motionStyles = {
    scale: 'transition-[background-color,color,border-color,transform,opacity] duration-instant ease-standard active:scale-[0.98]',
    paint: 'transition-[background-color,color,border-color] duration-instant ease-standard',
  };

  // Main-window controls sit at 28–36 px; icon-only targets never drop below 28 px.
  const sizeStyles = {
    xs: 'h-7 px-2 text-caption font-medium rounded-lg gap-1',
    sm: 'h-8 px-2.5 text-meta font-medium rounded-lg gap-1.5',
    md: 'h-9 px-3.5 text-body font-medium rounded-lg gap-2',
    icon: 'h-8 w-8 rounded-lg flex items-center justify-center',
  };
</script>

<button
  {type}
  {...rest}
  class={cn(
    'inline-flex items-center justify-center whitespace-nowrap font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 disabled:pointer-events-none disabled:opacity-45 select-none',
    motionStyles[motion],
    variantStyles[variant],
    sizeStyles[size],
    className,
  )}
>
  {#if children}
    {@render children()}
  {/if}
</button>
