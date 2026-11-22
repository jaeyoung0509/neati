<script lang="ts">
  import { onMount } from 'svelte';
  import type { AiProfileTool, AiToolProfile, AiToolProfileSuggestion } from '../models/types';
  import { settingsStore } from '../stores/settings.svelte';
  import { tauriSuggestAiToolProfiles } from '../utils/tauri';
  import {
    AI_PROFILE_TOOLS,
    MAX_AI_TOOL_PROFILES,
    aiProfileToolLabel,
    hasProfile,
    profileFolderName,
  } from '../utils/aiToolProfiles';
  import Button from './Button.svelte';

  const id = $props.id();
  let profiles = $derived(settingsStore.settings.ai_tool_profiles ?? []);
  let suggestions = $state<AiToolProfileSuggestion[]>([]);
  let tool = $state<AiProfileTool>('claude');
  let path = $state('');
  let busy = $state(false);
  let error = $state('');
  let atLimit = $derived(profiles.length >= MAX_AI_TOOL_PROFILES);
  let openSuggestions = $derived(suggestions.filter((suggestion) => !hasProfile(profiles, suggestion)));

  async function loadSuggestions() {
    try {
      suggestions = await tauriSuggestAiToolProfiles();
    } catch {
      // Suggestions are a convenience; the manual field still works.
      suggestions = [];
    }
  }
  onMount(() => { void loadSuggestions(); });

  /** The backend validates the folder; a refusal keeps the previous list. */
  async function save(next: AiToolProfile[]): Promise<boolean> {
    busy = true;
    error = '';
    const saved = await settingsStore.save({ ai_tool_profiles: next });
    busy = false;
    if (!saved) {
      error = settingsStore.error ?? 'The profile could not be saved.';
      settingsStore.error = null;
    }
    return saved;
  }

  async function add(candidate: AiToolProfile) {
    const trimmed = { tool: candidate.tool, path: candidate.path.trim() };
    if (!trimmed.path || atLimit || hasProfile(profiles, trimmed)) return;
    if (await save([...profiles, trimmed])) path = '';
  }

  async function remove(target: AiToolProfile) {
    await save(profiles.filter((profile) => !(profile.tool === target.tool && profile.path === target.path)));
  }
</script>

<div class="space-y-3 text-meta" data-ai-tool-profiles>
  <p class="text-muted-foreground">
    If you keep one configuration folder per account, add each extra folder here. neati then measures its cache, temporary and log folders like the default profile's. Extra profiles are reported for review and are never cleaned automatically.
  </p>

  {#if profiles.length > 0}
    <ul class="divide-y divide-border rounded-xl border border-border">
      {#each profiles as profile (profile.tool + ':' + profile.path)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <div class="min-w-0">
            <div class="text-foreground">{aiProfileToolLabel(profile.tool)} · {profileFolderName(profile.path)}</div>
            <div class="truncate font-mono text-caption text-muted-foreground" title={profile.path}>{profile.path}</div>
          </div>
          <Button size="sm" variant="ghost" disabled={busy} aria-label={`Remove ${aiProfileToolLabel(profile.tool)} profile ${profileFolderName(profile.path)}`} onclick={() => void remove(profile)}>Remove</Button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if openSuggestions.length > 0 && !atLimit}
    <div class="space-y-2 rounded-xl border border-border bg-secondary p-3">
      <div class="font-medium text-foreground">Folders that look like another profile</div>
      <ul class="space-y-1">
        {#each openSuggestions as suggestion (suggestion.tool + ':' + suggestion.path)}
          <li class="flex items-center justify-between gap-3">
            <span class="min-w-0 truncate"><span class="text-foreground">{aiProfileToolLabel(suggestion.tool)}</span> <span class="font-mono text-caption text-muted-foreground">{suggestion.path}</span></span>
            <Button size="sm" variant="outline" disabled={busy} aria-label={`Add ${suggestion.path}`} onclick={() => void add(suggestion)}>Add</Button>
          </li>
        {/each}
      </ul>
      <p class="text-caption text-muted-foreground">Suggested by folder name only. Nothing inside is read until you add it.</p>
    </div>
  {/if}

  <form class="flex flex-wrap items-end gap-2" onsubmit={(event) => { event.preventDefault(); void add({ tool, path }); }}>
    <label class="space-y-1">
      <span class="block text-muted-foreground">Tool</span>
      <select bind:value={tool} disabled={busy || atLimit} class="h-8 rounded-md border border-border-strong bg-card px-2 text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {#each AI_PROFILE_TOOLS as option (option.id)}<option value={option.id}>{option.label}</option>{/each}
      </select>
    </label>
    <label class="min-w-0 flex-1 space-y-1" for={id + '-path'}>
      <span class="block text-muted-foreground">Profile folder (full path)</span>
      <input
        id={id + '-path'}
        type="text"
        bind:value={path}
        disabled={busy || atLimit}
        placeholder="/Users/you/.claude-work"
        autocomplete="off"
        spellcheck="false"
        class="h-8 w-full rounded-md border border-border-strong bg-card px-2 font-mono text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
    </label>
    <Button type="submit" size="sm" disabled={busy || atLimit || path.trim().length === 0}>{busy ? 'Saving…' : 'Add profile'}</Button>
  </form>

  {#if atLimit}<p class="text-muted-foreground">{MAX_AI_TOOL_PROFILES} extra profiles is the limit. Remove one to add another.</p>{/if}
  {#if error}<p role="alert" class="text-destructive">{error}</p>{/if}
  <p class="text-caption text-muted-foreground">A profile must be a real folder inside your home folder. Rescan in Clean to see it.</p>
</div>
