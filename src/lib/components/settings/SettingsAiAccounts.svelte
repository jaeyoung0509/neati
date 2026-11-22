<script lang="ts">
  import type { AiProviderId, ProviderId } from '../../models/types';
  import { settingsStore } from '../../stores/settings.svelte';
  import { usageStore } from '../../stores/usage.svelte';
  import Card from '../Card.svelte';
  import Checkbox from '../Checkbox.svelte';
  import ReorderControls from '../ReorderControls.svelte';
  import { GripVertical, Users } from '@lucide/svelte';

  const FALLBACK_ACCOUNT_PROVIDER_IDS: readonly ProviderId[] = [
    'codex',
    'claude',
    'opencode',
    'openrouter',
    'antigravity',
    'cursor',
    'grok-build',
    'xai-api',
    'openai-api',
    'anthropic-api',
    'muse-code',
    'meta-model-api',
    'mistral-api',
    'fireworks-api',
  ];


  let accountProviderOptions = $derived(
    usageStore.accountProviderOptions.length > 0
      ? usageStore.accountProviderOptions
      : FALLBACK_ACCOUNT_PROVIDER_IDS.map((id) => ({
          id,
          label: id === 'grok-build' ? 'Grok Build' : id.split('-').map(s => s.charAt(0).toUpperCase() + s.slice(1)).join(' '),
          description: '',
        }))
  );


  let settings = $derived(settingsStore.settings);
  let draggedAccountProvider = $state<AiProviderId | null>(null);
  let dragOverAccountProvider = $state<AiProviderId | null>(null);

  function orderedAccountProviders() {
    const selected = settings.ai_accounts_quota_providers
      .map((id) => accountProviderOptions.find((option) => option.id === id))
      .filter((option): option is (typeof accountProviderOptions)[number] => Boolean(option));
    return [...selected, ...accountProviderOptions.filter((option) => !settings.ai_accounts_quota_providers.includes(option.id))];
  }
</script>

<!-- AI Accounts & Quota Customization -->
<div class="space-y-3">
  <div>
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      AI accounts and usage
    </h3>
    <p class="text-meta text-muted-foreground mt-1">
      Choose which account providers neati checks and displays. Disabled providers are not queried.
    </p>
  </div>
  <Card class="p-4 bg-card space-y-3">
    <div class="flex items-center gap-2 text-meta font-medium text-foreground pb-1">
      <Users size={14} /> Provider order
    </div>
    {#each orderedAccountProviders() as provider (provider.id)}
      {@const enabled = settings.ai_accounts_quota_providers.includes(provider.id)}
      {@const enabledIndex = settings.ai_accounts_quota_providers.indexOf(provider.id)}
      <div
        role="listitem"
        draggable={enabled}
        ondragstart={() => {
          if (enabled) draggedAccountProvider = provider.id;
        }}
        ondragover={(e) => {
          if (enabled && draggedAccountProvider) {
            e.preventDefault();
            dragOverAccountProvider = provider.id;
          }
        }}
        ondragleave={() => {
          if (dragOverAccountProvider === provider.id) dragOverAccountProvider = null;
        }}
        ondrop={(e) => {
          e.preventDefault();
          if (draggedAccountProvider && draggedAccountProvider !== provider.id) {
            settingsStore.reorderAccountsQuotaProviders(draggedAccountProvider, provider.id);
          }
          draggedAccountProvider = null;
          dragOverAccountProvider = null;
        }}
        ondragend={() => {
          draggedAccountProvider = null;
          dragOverAccountProvider = null;
        }}
        class="flex items-center gap-3 rounded-lg border px-3 py-2.5 transition-[background-color,border-color,opacity,transform] {enabled ? 'cursor-grab active:cursor-grabbing bg-card' : 'opacity-60 bg-muted/20'} {dragOverAccountProvider === provider.id ? 'border-primary bg-primary/5 scale-[1.01]' : 'border-border/60'} {draggedAccountProvider === provider.id ? 'opacity-40' : ''}"
      >
        <GripVertical size={14} class="text-muted-foreground/60 shrink-0 select-none {enabled ? 'hover:text-foreground' : 'opacity-20'}" />
        <Checkbox
          checked={enabled}
          disabled={enabled && settings.ai_accounts_quota_providers.length === 1}
          onchange={() => settingsStore.toggleAccountsQuotaProvider(provider.id)}
          ariaLabel={`Collect and show ${provider.label} account usage`}
        />
        <div class="min-w-0 flex-1 select-none">
          <div class="text-meta font-medium text-foreground">{provider.label}</div>
          <div class="text-caption text-muted-foreground">{provider.description}</div>
        </div>
        {#if enabled}
          <ReorderControls
            label={provider.label}
            index={enabledIndex}
            count={settings.ai_accounts_quota_providers.length}
            onMove={(direction) => settingsStore.moveAccountsQuotaProvider(provider.id, direction)}
          />
        {/if}
      </div>
    {/each}
  </Card>
</div>
