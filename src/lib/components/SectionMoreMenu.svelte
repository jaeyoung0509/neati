<script lang="ts">
  import { ChevronDown } from '@lucide/svelte';

  /**
   * The section's secondary views (#519): one menu instead of more tabs. A
   * native disclosure keeps it keyboard reachable without a custom focus trap.
   */
  interface MenuItem {
    id: string;
    label: string;
    badge?: string;
    disabled?: boolean;
    title?: string;
  }

  let {
    items,
    activeId,
    sectionLabel,
    onSelect,
  }: {
    items: MenuItem[];
    activeId: string;
    sectionLabel: string;
    onSelect: (id: string) => void;
  } = $props();

  let menu = $state<HTMLDetailsElement>();

  function choose(id: string) {
    if (menu) menu.open = false;
    onSelect(id);
  }

  function closeOnEscape(event: KeyboardEvent) {
    if (event.key === 'Escape' && menu?.open) {
      menu.open = false;
      menu.querySelector('summary')?.focus();
    }
  }

  function closeOnOutsideClick(event: MouseEvent) {
    if (menu?.open && !menu.contains(event.target as Node)) menu.open = false;
  }
</script>

<svelte:window onclick={closeOnOutsideClick} />

<svelte:document onkeydown={closeOnEscape} />

<details bind:this={menu} class="relative">
  <summary
    class="flex cursor-pointer list-none items-center gap-1 rounded-md px-2 py-1.5 text-meta text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden"
    aria-label={`More ${sectionLabel} views`}
    title={`More ${sectionLabel} views`}
  >
    More
    <ChevronDown size={13} aria-hidden="true" />
  </summary>
  <ul
    class="absolute right-0 z-20 mt-1 min-w-44 rounded-lg border border-border bg-card p-1 shadow-lg"
    aria-label={`More ${sectionLabel} views`}
  >
    {#each items as item (item.id)}
      <li>
        <button
          type="button"
          class={`flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-meta hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 ${item.id === activeId ? 'font-semibold text-foreground' : 'text-foreground'}`}
          aria-current={item.id === activeId ? 'page' : undefined}
          disabled={item.disabled}
          title={item.title}
          onclick={() => choose(item.id)}
        >
          <span>{item.label}</span>
          {#if item.badge}
            <span class="rounded border border-border px-1 text-caption text-muted-foreground">{item.badge}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
</details>
