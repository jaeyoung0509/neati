<script lang="ts">
  interface Props {
    checked: boolean;
    disabled?: boolean;
    color?: string;
    ariaLabel: string;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = false,
    disabled = false,
    color = 'peer-checked:bg-success',
    ariaLabel,
    onchange,
  }: Props = $props();

  function handleChange(event: Event) {
    if (disabled) return;
    const target = event.target as HTMLInputElement;
    onchange?.(target.checked);
  }
</script>

<label
  class="relative inline-flex items-center select-none {disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}"
>
  <input
    type="checkbox"
    {checked}
    {disabled}
    aria-label={ariaLabel}
    onchange={handleChange}
    class="sr-only peer"
  />
  <div
    class="w-9 h-5 border border-border-strong bg-secondary peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-1 peer-focus-visible:ring-offset-background rounded-full transition-colors duration-140 ease-out {color} relative"
  >
    <div
      class="absolute top-[1px] left-[1px] rounded-full h-4 w-4 transition-transform duration-140 ease-[cubic-bezier(0.16,1,0.3,1)] {checked ? 'translate-x-4 bg-success-foreground' : 'translate-x-0 bg-muted-foreground'}"
    ></div>
  </div>
</label>
