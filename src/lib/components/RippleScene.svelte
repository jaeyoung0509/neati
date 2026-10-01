<script lang="ts">
  import { onMount } from 'svelte';
  import { observeMotion } from '../utils/motionVisibility';
  let scene: SVGSVGElement;
  let allowed = $state(false);
  const id = $props.id();
  onMount(() => observeMotion(scene, (next) => { allowed = next; }));
  const ripples = [0, 1, 2, 3, 4];
  const particles = [{ x: 85, y: 47 }, { x: 162, y: 81 }, { x: 224, y: 38 }, { x: 284, y: 102 }, { x: 340, y: 55 }, { x: 384, y: 119 }];
</script>

<svg bind:this={scene} class="ripple-scene" viewBox="0 0 440 180" aria-hidden="true" focusable="false" style:--ripple-play={allowed ? 'running' : 'paused'}>
  <defs>
    <linearGradient id={`${id}-water`}><stop stop-color="hsl(var(--cosmic-ice))" stop-opacity="0" /><stop offset="0.45" stop-color="hsl(var(--cosmic-line))" stop-opacity="0.65" /><stop offset="1" stop-color="hsl(var(--cosmic-ice))" stop-opacity="0.1" /></linearGradient>
  </defs>
  <g class="ripple-lines" fill="none" stroke={`url(#${id}-water)`} stroke-width="0.8">
    {#each ripples as index}
      <path d={`M -35 ${76 + index * 13} C 60 ${-8 + index * 10}, 124 ${168 + index * 8}, 240 ${92 + index * 12} S 370 ${28 + index * 16}, 480 ${83 + index * 9}`} />
    {/each}
  </g>
  <g class="ripple-particles" fill="hsl(var(--cosmic-line))">
    {#each particles as particle, index}<circle cx={particle.x} cy={particle.y} r={index % 2 ? 1.4 : 1} opacity={index % 2 ? 0.3 : 0.45} />{/each}
  </g>
</svg>

<style>
  .ripple-scene { display: block; width: 100%; height: auto; pointer-events: none; }
  .ripple-lines, .ripple-particles { animation: drift 22s ease-in-out infinite alternate; animation-play-state: var(--ripple-play, paused); }
  .ripple-particles { animation-duration: 18s; animation-direction: alternate-reverse; }
  @keyframes drift { from { transform: translateY(-4px); } to { transform: translateY(4px); } }
  @media (prefers-reduced-motion: reduce) { .ripple-lines, .ripple-particles { animation: none; } }
  @media (prefers-reduced-transparency: reduce) { .ripple-scene { display: none; } }
</style>
