<script lang="ts">
  import { Pencil, Trash2 } from '@lucide/svelte';
  import type { AwakeRule } from '../models/types';
  import { awakeStore } from '../stores/awake.svelte';
  import { settingsStore } from '../stores/settings.svelte';
  import LoadingIndicator from './LoadingIndicator.svelte';
  import Switch from './Switch.svelte';

  /**
   * Edit, delete and on/off for one Keep Awake rule. While any Keep Awake
   * change is saving (or before settings have loaded) these controls are
   * disabled, so a second click cannot race the first (#491).
   */
  let { rule, label, onEdit }: {
    rule: AwakeRule;
    /** Accessible noun phrase, e.g. `Codex rule` or `Codex legacy rule`. */
    label: string;
    onEdit: () => void;
  } = $props();

  let locked = $derived(awakeStore.isLoading || !settingsStore.hasLoaded);
  let deleting = $derived(
    awakeStore.pending?.kind === 'rule' && awakeStore.pending.ruleId === rule.id && awakeStore.pending.action === 'delete'
  );
  let toggling = $derived(
    awakeStore.pending?.kind === 'rule' && awakeStore.pending.ruleId === rule.id && awakeStore.pending.action === 'toggle'
  );
  const iconButton = 'p-1.5 rounded-lg text-muted-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50';
</script>

<div class="flex items-center gap-2 shrink-0" aria-busy={deleting || toggling}>
  <button type="button" onclick={onEdit} disabled={!settingsStore.hasLoaded} class="{iconButton} hover:text-foreground hover:bg-secondary/60" title={`Edit ${label}`} aria-label={`Edit ${label}`}>
    <Pencil size={14} aria-hidden="true" />
  </button>
  <button type="button" onclick={() => void awakeStore.deleteRule(rule.id)} disabled={locked} class="{iconButton} hover:text-destructive hover:bg-destructive/10" title={deleting ? `Deleting ${label}…` : `Delete ${label}`} aria-label={deleting ? `Deleting ${label}…` : `Delete ${label}`}>
    {#if deleting}
      <LoadingIndicator size="xs" />
    {:else}
      <Trash2 size={14} aria-hidden="true" />
    {/if}
  </button>
  {#if toggling}
    <span class="sr-only" role="status">Saving {label}…</span>
  {/if}
  <Switch checked={rule.enabled} disabled={locked} onchange={() => void awakeStore.toggleRule(rule.id)} ariaLabel={`Toggle ${label}`} />
</div>
