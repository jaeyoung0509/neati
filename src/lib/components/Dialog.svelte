<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLDialogAttributes } from 'svelte/elements';
  import { cn } from '../utils/cn';
  import { modalDialog } from '../utils/modalDialog';

  /**
   * The one modal surface. Render it inside an `{#if}`: mounting opens it with
   * `showModal()`, unmounting closes it and returns focus. Escape calls
   * `onCancel`; the owner decides whether cancelling is allowed while busy.
   */
  interface Props extends Omit<HTMLDialogAttributes, 'class' | 'children' | 'oncancel' | 'open' | 'tabindex'> {
    onCancel: () => void;
    /** Selector inside the dialog that receives focus on open; the dialog itself by default. */
    initialFocus?: string;
    returnFocusTarget?: () => HTMLElement | null;
    restoreFocusOnClose?: () => boolean;
    size?: 'sm' | 'md' | 'lg' | 'xl';
    /** `compact` is for the Quick Panel, whose window is narrower. */
    density?: 'default' | 'compact';
    /** `flex` hands scrolling to the content so a header and footer can stay fixed. */
    layout?: 'scroll' | 'flex';
    tone?: 'default' | 'destructive';
    class?: string;
    children: Snippet;
  }

  let {
    onCancel,
    initialFocus,
    returnFocusTarget,
    restoreFocusOnClose,
    size = 'lg',
    density = 'default',
    layout = 'scroll',
    tone = 'default',
    class: className = '',
    children,
    ...rest
  }: Props = $props();

  const sizes = { sm: 'max-w-sm', md: 'max-w-md', lg: 'max-w-lg', xl: 'max-w-xl' };
  const densities = {
    default: 'w-[calc(100%-2rem)] max-h-[calc(100%-2rem)] p-5',
    compact: 'w-[calc(100%-1.5rem)] max-h-[calc(100%-1.5rem)] p-4',
  };
  const layouts = { scroll: 'overflow-y-auto scroll-stable', flex: 'overflow-hidden p-0 open:flex open:flex-col' };
  const tones = { default: 'border-border', destructive: 'border-destructive/40' };
</script>

<dialog
  use:modalDialog={{ onCancel, initialFocus, returnFocusTarget, restoreFocusOnClose }}
  tabindex="-1"
  aria-modal="true"
  data-dialog
  class={cn(
    'm-auto rounded-2xl border bg-card text-foreground shadow-xl backdrop:bg-foreground/30 focus:outline-none [overflow-wrap:anywhere]',
    densities[density],
    sizes[size],
    layouts[layout],
    tones[tone],
    className,
  )}
  {...rest}
>
  {@render children()}
</dialog>
