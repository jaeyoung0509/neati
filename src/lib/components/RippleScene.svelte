<script lang="ts">
  import { onMount } from 'svelte';
  import { observeMotion } from '../utils/motionVisibility';
  let { activity = 'idle' }: { activity?: 'idle' | 'working' | 'stopping' } = $props();
  let scene: SVGSVGElement;
  let allowed = $state(false);
  const id = $props.id();
  onMount(() => observeMotion(scene, (next) => { allowed = next; }));
  const ripples = [0, 1, 2];
</script>

<svg bind:this={scene} class="ripple-scene" data-activity={activity} data-moving={allowed && activity !== 'stopping'} viewBox="0 0 440 180" aria-hidden="true" focusable="false">
  <defs>
    <linearGradient id={`${id}-water`}><stop stop-color="hsl(var(--cosmic-ice))" stop-opacity="0" /><stop offset="0.4" stop-color="hsl(var(--cosmic-line))" stop-opacity="0.8" /><stop offset="1" stop-color="hsl(var(--cosmic-ice))" stop-opacity="0.2" /></linearGradient>
  </defs>
  <g class="ripple-lines" fill="none" stroke={`url(#${id}-water)`} stroke-width="1.1" stroke-linecap="round">
    {#each ripples as index}
      <path d={`M -35 ${98 + index * 14} C 76 ${8 + index * 9}, 120 ${166 + index * 10}, 242 ${92 + index * 11} S 386 ${26 + index * 17}, 480 ${72 + index * 15}`} />
    {/each}
  </g>
  <g class="ripple-echo" fill="none" stroke={`url(#${id}-water)`} stroke-width="0.75" opacity="0.45" stroke-linecap="round">
    <path d="M 50 140 C 145 74, 192 150, 276 112 S 395 63, 468 106" />
    <path d="M 62 154 C 153 94, 199 164, 282 130 S 398 86, 468 125" />
  </g>
</svg>

<style>
  .ripple-scene { display: block; width: 100%; height: auto; pointer-events: none; }
  [data-moving='true'] .ripple-lines { animation: water-flow 18s ease-in-out infinite alternate; }
  [data-moving='true'] .ripple-echo { animation: water-flow 22s ease-in-out infinite alternate-reverse; }
  [data-activity='working'][data-moving='true'] .ripple-lines { animation-duration: 7s; }
  [data-activity='working'][data-moving='true'] .ripple-echo { animation-duration: 9s; }
  @keyframes water-flow { from { transform: translate(-6px, 4px); opacity: 0.7; } to { transform: translate(6px, -4px); opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .ripple-lines, .ripple-echo { animation: none !important; } }
  @media (prefers-reduced-transparency: reduce) { .ripple-scene { display: none; } }
</style>
