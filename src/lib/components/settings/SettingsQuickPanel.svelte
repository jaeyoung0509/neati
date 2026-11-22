<script lang="ts">
  import type { AiProviderId, ProviderId, QuickPanelSection } from '../../models/types';
  import { settingsStore } from '../../stores/settings.svelte';
  import { usageStore } from '../../stores/usage.svelte';
  import { platformContextStore } from '../../stores/platformContext.svelte';
  import { titleCaseLabel } from '../../utils/platformCopy';
  import Card from '../Card.svelte';
  import Checkbox from '../Checkbox.svelte';
  import ReorderControls from '../ReorderControls.svelte';
  import { GripVertical, PanelTop, Sparkles } from '@lucide/svelte';

  const sectionOptions: { id: QuickPanelSection; label: string; description: string }[] = [
    { id: 'cleanup', label: 'Quick Clean', description: 'Safe reclaimable storage and clean action.' },
    { id: 'cpu', label: 'CPU', description: 'Current measured system-wide usage.' },
    { id: 'battery', label: 'Battery', description: 'Charge level and actual charging state.' },
    { id: 'storage', label: 'Storage', description: 'Primary disk capacity and usage.' },
    { id: 'memory', label: 'Memory', description: 'Memory pressure and current usage.' },
    { id: 'categories', label: 'Storage Categories', description: 'AI, developer, container, model, and system totals.' },
    { id: 'agent_activity', label: 'AI & Agents', description: 'Active AI agent sessions and account token limits.' },
    { id: 'awake', label: 'Keep Awake', description: 'Current session and a one-hour control.' },
  ];
  const FALLBACK_QUICK_PANEL_OPTIONS: { id: ProviderId; label: string }[] = [
    { id: 'codex', label: 'Codex' },
    { id: 'claude', label: 'Claude Code' },
    { id: 'opencode', label: 'OpenCode' },
    { id: 'openrouter', label: 'OpenRouter' },
    { id: 'antigravity', label: 'Antigravity' },
  ];

  let quickPanelProviderOptions = $derived(
    usageStore.quickPanelProviderOptions.length > 0
      ? usageStore.quickPanelProviderOptions
      : FALLBACK_QUICK_PANEL_OPTIONS
  );

  let settings = $derived(settingsStore.settings);
  let draggedSection = $state<QuickPanelSection | null>(null);
  let dragOverSection = $state<QuickPanelSection | null>(null);
  let draggedProvider = $state<AiProviderId | null>(null);
  let dragOverProvider = $state<AiProviderId | null>(null);

  function orderedSections() {
    const selected = settings.quick_panel_sections
      .map((id) => sectionOptions.find((option) => option.id === id))
      .filter((option): option is (typeof sectionOptions)[number] => Boolean(option));
    return [...selected, ...sectionOptions.filter((option) => !settings.quick_panel_sections.includes(option.id))];
  }

  function orderedQuickPanelProviders() {
    const selected = settings.quick_panel_ai_providers
      .map((id) => quickPanelProviderOptions.find((option) => option.id === id))
      .filter((option): option is (typeof quickPanelProviderOptions)[number] => Boolean(option));
    return [...selected, ...quickPanelProviderOptions.filter((option) => !settings.quick_panel_ai_providers.includes(option.id))];
  }

</script>

<!-- Quick Panel Customization -->
<div class="space-y-3">
  <div>
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      {platformContextStore.quickPanelSurfaceLabel
        ? `${titleCaseLabel(platformContextStore.quickPanelSurfaceLabel)} Quick Panel`
        : 'Quick Panel'}
    </h3>
    <p class="text-meta text-muted-foreground mt-1">
      Choose what appears below the {platformContextStore.quickPanelSurfaceLabel ?? 'quick panel'} icon. Drag or use the arrow buttons to set priority.
    </p>
  </div>
  <Card class="p-4 bg-card space-y-5">
    <div class="space-y-2">
      <div class="flex items-center gap-2 text-meta font-medium text-foreground">
        <PanelTop size={14} /> Sections
      </div>
      {#each orderedSections() as option (option.id)}
        {@const enabled = settings.quick_panel_sections.includes(option.id)}
        {@const enabledIndex = settings.quick_panel_sections.indexOf(option.id)}
        <div
          role="listitem"
          draggable={enabled}
          ondragstart={() => {
            if (enabled) draggedSection = option.id;
          }}
          ondragover={(e) => {
            if (enabled && draggedSection) {
              e.preventDefault();
              dragOverSection = option.id;
            }
          }}
          ondragleave={() => {
            if (dragOverSection === option.id) dragOverSection = null;
          }}
          ondrop={(e) => {
            e.preventDefault();
            if (draggedSection && draggedSection !== option.id) {
              settingsStore.reorderQuickPanelSections(draggedSection, option.id);
            }
            draggedSection = null;
            dragOverSection = null;
          }}
          ondragend={() => {
            draggedSection = null;
            dragOverSection = null;
          }}
          class="flex items-center gap-3 rounded-lg border px-3 py-2.5 transition-[background-color,border-color,opacity,transform] {enabled ? 'cursor-grab active:cursor-grabbing bg-card' : 'opacity-60 bg-muted/20'} {dragOverSection === option.id ? 'border-primary bg-primary/5 scale-[1.01]' : 'border-border/60'} {draggedSection === option.id ? 'opacity-40' : ''}"
        >
          <GripVertical size={14} class="text-muted-foreground/60 shrink-0 select-none {enabled ? 'hover:text-foreground' : 'opacity-20'}" />
          <Checkbox
            checked={enabled}
            disabled={enabled && settings.quick_panel_sections.length === 1}
            onchange={() => settingsStore.toggleQuickPanelSection(option.id)}
            ariaLabel={`Show ${option.label} in quick panel`}
          />
          <div class="min-w-0 flex-1 select-none">
            <div class="text-meta font-medium text-foreground">{option.label}</div>
            <div class="text-caption text-muted-foreground">{option.description}</div>
          </div>
          {#if enabled}
            <ReorderControls
              label={option.label}
              index={enabledIndex}
              count={settings.quick_panel_sections.length}
              onMove={(direction) => settingsStore.moveQuickPanelSection(option.id, direction)}
            />
          {/if}
        </div>
      {/each}
    </div>

    <div class="space-y-2 pt-4 border-t border-border/60">
      <div class="flex items-center gap-2 text-meta font-medium text-foreground">
        <Sparkles size={14} /> AI Provider Priority
      </div>
      <p class="text-caption text-muted-foreground">Show usage for accounts enabled under AI accounts and usage, in this order. Turning usage off hides its quota row; observed local sessions remain separate.</p>
      {#each orderedQuickPanelProviders() as provider (provider.id)}
        {@const enabled = settings.quick_panel_ai_providers.includes(provider.id)}
        {@const enabledIndex = settings.quick_panel_ai_providers.indexOf(provider.id)}
        <div
          role="listitem"
          draggable={enabled}
          ondragstart={() => {
            if (enabled) draggedProvider = provider.id;
          }}
          ondragover={(e) => {
            if (enabled && draggedProvider) {
              e.preventDefault();
              dragOverProvider = provider.id;
            }
          }}
          ondragleave={() => {
            if (dragOverProvider === provider.id) dragOverProvider = null;
          }}
          ondrop={(e) => {
            e.preventDefault();
            if (draggedProvider && draggedProvider !== provider.id) {
              settingsStore.reorderQuickPanelProviders(draggedProvider, provider.id);
            }
            draggedProvider = null;
            dragOverProvider = null;
          }}
          ondragend={() => {
            draggedProvider = null;
            dragOverProvider = null;
          }}
          class="flex items-center gap-3 rounded-lg border px-3 py-2 transition-[background-color,border-color,opacity,transform] {enabled ? 'cursor-grab active:cursor-grabbing bg-card' : 'opacity-60 bg-muted/20'} {dragOverProvider === provider.id ? 'border-primary bg-primary/5 scale-[1.01]' : 'border-border/60'} {draggedProvider === provider.id ? 'opacity-40' : ''}"
        >
          <GripVertical size={14} class="text-muted-foreground/60 shrink-0 select-none {enabled ? 'hover:text-foreground' : 'opacity-20'}" />
          <Checkbox
            checked={enabled}
            disabled={!settings.ai_accounts_quota_providers.includes(provider.id)}
            onchange={() => settingsStore.toggleQuickPanelProvider(provider.id)}
            ariaLabel={`Show ${provider.label} usage`}
          />
          <span class="flex-1 text-meta font-medium text-foreground select-none">{provider.label}</span>
          {#if enabled}
            <ReorderControls
              label={provider.label}
              index={enabledIndex}
              count={settings.quick_panel_ai_providers.length}
              onMove={(direction) => settingsStore.moveQuickPanelProvider(provider.id, direction)}
            />
          {/if}
        </div>
      {/each}
    </div>
  </Card>
</div>
