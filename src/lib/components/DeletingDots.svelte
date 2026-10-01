<script lang="ts">
  import { onMount } from 'svelte';
  import { observeMotion } from '../utils/motionVisibility';
  interface Props {
    size?: 'xs' | 'sm' | 'md';
    color?: string;
    class?: string;
    active?: boolean;
  }

  let { size = 'md', color = 'bg-current', class: className = '', active = true }: Props = $props();

  let dotSize = $derived(size === 'xs' ? 'w-1 h-1' : size === 'sm' ? 'w-[5px] h-[5px]' : 'w-1.5 h-1.5');
  let element: HTMLSpanElement;
  let allowed = $state(false);
  onMount(() => observeMotion(element, (next) => { allowed = next; }));
</script>

<span bind:this={element} class="inline-flex items-center gap-1 {className}" aria-hidden="true">
  <span class="{dotSize} rounded-full {color} animate-bounce-dot-1" style:animation-play-state={allowed && active ? 'running' : 'paused'}></span>
  <span class="{dotSize} rounded-full {color} animate-bounce-dot-2" style:animation-play-state={allowed && active ? 'running' : 'paused'}></span>
  <span class="{dotSize} rounded-full {color} animate-bounce-dot-3" style:animation-play-state={allowed && active ? 'running' : 'paused'}></span>
</span>
