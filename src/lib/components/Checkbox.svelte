<script lang="ts">
  import { Check, Minus } from '@lucide/svelte';

  interface Props {
    checked: boolean;
    indeterminate?: boolean;
    disabled?: boolean;
    ariaLabel: string;
    label?: string;
    class?: string;
    title?: string;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = false,
    indeterminate = false,
    disabled = false,
    ariaLabel,
    label,
    class: className = '',
    title,
    onchange,
  }: Props = $props();

  function handleChange(event: Event) {
    if (disabled) return;
    const target = event.target as HTMLInputElement;
    onchange?.(target.checked);
  }
</script>

<label
  {title}
  class="relative inline-flex items-center justify-center select-none {disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'} {className}"
>
  <input
    type="checkbox"
    {checked}
    {indeterminate}
    {disabled}
    {title}
    aria-label={ariaLabel}
    onchange={handleChange}
    class="sr-only peer"
  />
  <div
    class="h-4 w-4 rounded-[4px] border transition-colors duration-140 flex items-center justify-center peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-1 peer-focus-visible:ring-offset-background {checked || indeterminate
      ? 'bg-success border-success text-success-foreground'
      : 'border-border-strong bg-card hover:border-foreground text-transparent'}"
  >
    {#if indeterminate}
      <Minus size={11} strokeWidth={3} aria-hidden="true" />
    {:else if checked}
      <Check size={11} strokeWidth={3} class="stroke-success-foreground" />
    {/if}
  </div>
  {#if label}<span class="text-body font-medium">{label}</span>{/if}
</label>
