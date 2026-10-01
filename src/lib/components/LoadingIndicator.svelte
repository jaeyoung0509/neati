<script lang="ts">
  import { onMount } from 'svelte';
  import neatiMark from '../../../src-tauri/icons/neati-mark.svg';
  import { observeMotion } from '../utils/motionVisibility';

  let { size = 'md', active = true, class: className = '' }: {
    size?: 'xs' | 'sm' | 'md';
    active?: boolean;
    class?: string;
  } = $props();
  let element: HTMLSpanElement;
  let allowed = $state(false);
  const dimensions = { xs: 16, sm: 20, md: 28 };
  onMount(() => observeMotion(element, (next) => { allowed = next; }));
</script>

<!-- The canonical handwritten stroke, with one ripple; nearby text names the work. -->
<span bind:this={element} data-loading-indicator data-moving={allowed && active}
  class="neati-loading shrink-0 {className}" style:--loading-size={`${dimensions[size]}px`}
  aria-hidden="true">
  <span class="loading-stroke" style={`mask-image: url("${neatiMark}"); -webkit-mask-image: url("${neatiMark}")`}></span>
  <span class="loading-ripple"></span>
</span>

<style>
  .neati-loading { position: relative; display: inline-block; vertical-align: middle; width: var(--loading-size); height: var(--loading-size); }
  .loading-stroke { position: absolute; inset: 0; background: currentColor; mask-size: contain; mask-repeat: no-repeat; mask-position: center; -webkit-mask-size: contain; -webkit-mask-repeat: no-repeat; -webkit-mask-position: center; }
  .loading-ripple { position: absolute; left: 22%; right: 10%; bottom: 4%; height: 16%; border: 1px solid currentColor; border-radius: 50%; opacity: 0.3; }
  [data-moving='true'] .loading-stroke { animation: stroke-float 2.4s ease-in-out infinite; }
  [data-moving='true'] .loading-ripple { animation: ripple-out 2.4s ease-out infinite; transform-origin: center; }
  @keyframes stroke-float { 0%, 100% { transform: translateY(0); opacity: 0.8; } 45% { transform: translateY(-1px); opacity: 1; } }
  @keyframes ripple-out { 0%, 15% { transform: scale(0.8); opacity: 0.1; } 40% { opacity: 0.45; } 100% { transform: scale(1.25); opacity: 0; } }
  @media (prefers-reduced-motion: reduce), (prefers-reduced-transparency: reduce) { .loading-stroke, .loading-ripple { animation: none !important; } }
</style>
