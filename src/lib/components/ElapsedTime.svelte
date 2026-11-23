<script lang="ts">
  import { onMount } from 'svelte';
  import { observeWhileVisible } from '../utils/visiblePolling';
  import { elapsedSeconds, formatElapsed } from '../utils/cleanProgress';

  /**
   * Seconds since an operation started (#491). The clock belongs to the
   * mounted surface and stops in a hidden document. It is not a live region:
   * the operation's own status sentence is announced, not every second.
   */
  let { since, class: className = '' }: { since: number; class?: string } = $props();
  let now = $state(Date.now());
  onMount(() => observeWhileVisible(() => { now = Date.now(); }, 1000));
</script>

<span class="tabular-nums {className}" data-elapsed>{formatElapsed(elapsedSeconds(since, now))}</span>
