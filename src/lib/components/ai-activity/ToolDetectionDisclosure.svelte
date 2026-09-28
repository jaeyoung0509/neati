<script lang="ts">
  import ToolDetectionPanel from './ToolDetectionPanel.svelte';
  let expanded = $state(false);
</script>

<details class="rounded-xl border border-border bg-card" ontoggle={(event) => { expanded = event.currentTarget.open; }}>
  <summary class="cursor-pointer rounded-xl px-4 py-3 text-body font-medium hover:bg-secondary focus-visible:ring-2 focus-visible:ring-ring">
    Supported tools and detection status
  </summary>
  {#if expanded}
    <div class="border-t border-border p-4"><ToolDetectionPanel /></div>
  {/if}
</details>
