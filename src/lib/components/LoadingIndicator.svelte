<script lang="ts">
  import { onMount } from 'svelte';
  import { observeMotion } from '../utils/motionVisibility';
  import { handwrittenStatus, type LoadingWord, type LoadingTone, type LoadingMotion } from '../utils/handwrittenStatus';

  let { size = 'md', variant = 'dots', word = 'loading', tone = 'brand', motion = 'write', active = true, class: className = '' }: {
    size?: 'xs' | 'sm' | 'md';
    variant?: 'dots' | 'handwriting';
    word?: LoadingWord;
    tone?: LoadingTone;
    motion?: LoadingMotion;
    active?: boolean;
    class?: string;
  } = $props();
  let element: HTMLSpanElement;
  let allowed = $state(false);
  const instanceId = $props.id();
  const inkId = `${instanceId}-loading-ink`;
  const dimensions = { xs: [64, 24], sm: [76, 28], md: [104, 38] };
  const dotSizes = { xs: 4, sm: 5, md: 6 };
  const dotHeights = { xs: 16, sm: 20, md: 28 };
  let dotSize = $derived(dotSizes[size]);
  let artwork = $derived(handwrittenStatus[word]);
  let width = $derived(variant === 'dots' ? dotSize * 3 + 8 : Math.ceil(dimensions[size][0] * artwork.width / handwrittenStatus.loading.width));
  let height = $derived(variant === 'dots' ? dotHeights[size] : dimensions[size][1]);
  onMount(() => observeMotion(element, (next) => { allowed = next; }));
</script>

<!-- Nearby text owns the status. Handwriting is reserved for the main Cleanup scan. -->
<span bind:this={element} data-loading-indicator data-loading-variant={variant} data-loading-word={word} data-moving={allowed && active}
  data-tone={tone} data-motion={motion} class="neati-loading shrink-0 {className}"
  style:width={`${width}px`} style:height={`${height}px`} aria-hidden="true">
  {#if variant === 'dots'}
  <svg viewBox={`0 0 ${width} ${height}`} fill="currentColor" focusable="false" style:--dot-rest={`${height / 2}px`} style:--dot-rise={`${height / 2 - 4.5}px`}>
    {#each [0, 1, 2] as index}
      <circle class="loading-bounce-dot" cx={dotSize / 2 + index * (dotSize + 4)} cy={height / 2} r={dotSize / 2} style:animation-delay={`${index * 0.18}s`} />
    {/each}
  </svg>
  {:else}
  <svg viewBox={`0 0 ${artwork.width} 64`} fill="none" stroke-width="3.8"
    stroke-linecap="round" stroke-linejoin="round" focusable="false">
    <defs>
      <linearGradient id={inkId} x1="0" y1="0" x2={artwork.width} y2="0" gradientUnits="userSpaceOnUse">
        <stop offset="0" style="--ink-base: var(--primary); --ink-wave: var(--meter-end); --ink-delay: 0s" />
        <stop offset="0.52" style="--ink-base: var(--meter-middle); --ink-wave: var(--primary); --ink-delay: 0.16s" />
        <stop offset="1" style="--ink-base: var(--meter-end); --ink-wave: var(--primary); --ink-delay: 0.32s" />
      </linearGradient>
    </defs>
    <g stroke={tone === 'brand' ? `url(#${inkId})` : 'currentColor'}>
      <path class="loading-writing" pathLength="1" d={artwork.path} />
      <path class="loading-dot" pathLength="1" d={artwork.dots} stroke-width="5" />
      {#if tone === 'ink' && motion === 'flow'}
        <path class="loading-ink-pass" pathLength="1" d={artwork.path} stroke-width="5.2" />
      {/if}
    </g>
  </svg>
  {/if}
</span>

<style>
  .neati-loading { display: inline-flex; align-items: center; vertical-align: middle; flex-shrink: 0; }
  svg { width: 100%; height: 100%; overflow: visible; }
  /* SVG geometry restores the original bounce without compositor layers. */
  [data-moving='true'] .loading-bounce-dot { animation: loading-bounce-dot 1.2s ease-in-out infinite; }
  @keyframes loading-bounce-dot {
    0%, 80%, 100% { cy: var(--dot-rest); fill-opacity: 0.5; }
    40% { cy: var(--dot-rise); fill-opacity: 1; }
  }
  .loading-writing, .loading-dot { stroke-dasharray: 1; stroke-dashoffset: 0; }
  stop { stop-color: hsl(var(--ink-base)); }
  [data-moving='true'][data-motion='write'] .loading-writing { animation: loading-write 3.8s linear infinite; }
  [data-moving='true'][data-motion='write'] .loading-dot { animation: loading-dot 3.8s linear infinite; }
  [data-moving='true'][data-tone='brand'] stop { animation: ink-flow 3.8s ease-in-out infinite; animation-delay: var(--ink-delay); }
  .loading-ink-pass { stroke-dasharray: 0.12 1; stroke-opacity: 0; }
  [data-moving='true'] .loading-ink-pass { animation: ink-pass 3.8s linear infinite; }
  @keyframes ink-pass {
    0% { stroke-dashoffset: 0.12; stroke-opacity: 1; }
    88% { stroke-dashoffset: -1; stroke-opacity: 1; }
    89%, 100% { stroke-dashoffset: -1; stroke-opacity: 0; }
  }
  @keyframes ink-flow {
    0%, 62%, 94%, 100% { stop-color: hsl(var(--ink-base)); }
    72%, 80% { stop-color: hsl(var(--ink-wave)); }
  }
  @keyframes loading-write {
    0% { stroke-dashoffset: 1; stroke-opacity: 1; }
    64%, 88% { stroke-dashoffset: 0; stroke-opacity: 1; }
    98%, 100% { stroke-dashoffset: 0; stroke-opacity: 0; }
  }
  @keyframes loading-dot {
    0%, 64% { stroke-dashoffset: 1; stroke-opacity: 0; }
    68%, 88% { stroke-dashoffset: 0; stroke-opacity: 1; }
    98%, 100% { stroke-dashoffset: 0; stroke-opacity: 0; }
  }
  @media (prefers-reduced-motion: reduce), (prefers-reduced-transparency: reduce) {
    .loading-bounce-dot { animation: none !important; }
    .loading-writing, .loading-dot { animation: none !important; stroke-dashoffset: 0; stroke-opacity: 1; }
    stop { animation: none !important; }
    .loading-ink-pass { animation: none !important; stroke-opacity: 0; }
  }
</style>
