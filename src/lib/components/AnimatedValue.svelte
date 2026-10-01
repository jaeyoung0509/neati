<script lang="ts">
  import { onMount } from 'svelte';
  import { observeMotion } from '../utils/motionVisibility';
  import { createValueMotion } from '../utils/valueMotion';

  let { value, active = true, class: className = '' }: { value: string; active?: boolean; class?: string } = $props();
  let element: HTMLSpanElement;
  let allowed = $state(false);
  let controller = $state<ReturnType<typeof createValueMotion>>();
  onMount(() => {
    controller = createValueMotion(element);
    const dispose = observeMotion(element, (next) => { allowed = next; });
    return () => { dispose(); controller?.stop(); };
  });
  $effect(() => { controller?.update(value, allowed && active); });
</script>

<!-- The accessible and visible value are always the actual latest reading. -->
<span bind:this={element} data-animated-value class="inline-block whitespace-nowrap tabular-nums {className}">{value}</span>
