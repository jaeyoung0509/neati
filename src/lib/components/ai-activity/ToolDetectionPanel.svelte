<script lang="ts">
  import { onMount } from 'svelte';
  import type { AgentAdapterHealth } from '../../models/types';
  import { agentActivityStore } from '../../stores/agentActivity.svelte';
  import { formatTimeAgo } from '../../utils/format';
  import Badge from '../Badge.svelte';
  import Button from '../Button.svelte';

  let snapshot = $derived(agentActivityStore.snapshot);
  let loading = $derived(agentActivityStore.isLoading || agentActivityStore.isIntegrationsLoading);
  let actionFeedback = $state<string | null>(null);
  let actionError = $state(false);
  let removing = $state<string | null>(null);

  // Mounted only while the Settings disclosure is open. No timer or hidden polling.
  onMount(() => { void refresh(); });

  async function refresh() {
    await Promise.all([agentActivityStore.refresh(true), agentActivityStore.fetchIntegrations()]);
  }

  function status(adapter: AgentAdapterHealth) {
    switch (adapter.state) {
      case 'connected': return { label: 'Connected', variant: 'success' as const };
      case 'integration_available': return { label: 'Integration available', variant: 'outline' as const };
      case 'process_only': return { label: adapter.evidence === 'process_observed' ? 'Process observed' : 'Detection only', variant: 'secondary' as const };
      case 'not_installed': return { label: 'Not detected', variant: 'outline' as const };
      case 'version_unsupported': return { label: 'Unsupported version', variant: 'warning' as const };
      case 'partial': return { label: 'Limited detection', variant: 'warning' as const };
    }
  }

  function description(adapter: AgentAdapterHealth) {
    if (adapter.state === 'process_only') {
      return adapter.evidence === 'process_observed'
        ? 'A process was seen. Its current task and progress are unknown.'
        : 'Can check whether the tool is running, not what it is doing.';
    }
    if (adapter.state === 'not_installed') return 'Not found in supported locations.';
    return adapter.message;
  }

  async function removeLegacyMarker(toolId: string) {
    if (removing) return;
    removing = toolId;
    actionFeedback = null;
    actionError = false;
    try {
      actionFeedback = (await agentActivityStore.uninstallIntegration(toolId)).message;
    } catch (error) {
      actionError = true;
      actionFeedback = `Could not remove the legacy marker: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      removing = null;
    }
  }
</script>

<section aria-label="Supported tools and detection status" class="space-y-3">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="min-w-0 flex-1 text-meta text-muted-foreground">
      <p>Built-in detection, not installable plugins. Seeing a process does not mean it is working or using tokens.</p>
      {#if snapshot}
        <p class="mt-1">Last observation: {formatTimeAgo(snapshot.observed_at)}</p>
      {/if}
    </div>
    <Button variant="outline" size="sm" disabled={loading || removing !== null} onclick={refresh}>
      {loading ? 'Checking tools…' : 'Refresh detection'}
    </Button>
  </div>

  {#if agentActivityStore.error}
    <p role="alert" class="text-meta text-destructive">
      Detection failed. {snapshot ? 'Showing the last successful observation; it may be out of date.' : 'No observation is available.'} {agentActivityStore.error}
    </p>
  {/if}
  {#if agentActivityStore.integrationsError}
    <p role="alert" class="text-meta text-warning">Legacy integration status could not be refreshed. {agentActivityStore.integrationsError}</p>
  {/if}
  {#if loading}
    <p role="status" class="text-meta text-muted-foreground">Checking supported tools and legacy integration status…</p>
  {/if}

  {#if snapshot}
    {#if snapshot.partial_errors.length > 0}
      <p role="status" class="text-meta text-warning">Some detection checks were incomplete: {snapshot.partial_errors.join(' · ')}</p>
    {/if}
    {#if snapshot.adapters.length === 0}
      <p class="text-meta text-muted-foreground">No supported tool detection results were returned.</p>
    {:else}
      <ul class="divide-y divide-border rounded-xl border border-border bg-card">
        {#each snapshot.adapters as adapter (adapter.tool_id)}
          {@const stateLabel = status(adapter)}
          {@const integration = agentActivityStore.integrations.find(item => item.tool_id === adapter.tool_id)}
          <li class="min-w-0 space-y-2 px-4 py-3">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-body font-medium">{adapter.display_name}</span>
              <Badge variant={stateLabel.variant}>{stateLabel.label}</Badge>
            </div>
            <p class="break-words text-meta text-muted-foreground">{description(adapter)}</p>
            <details class="text-meta text-muted-foreground">
              <summary class="w-fit cursor-pointer rounded-sm hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">Technical details</summary>
              <div class="mt-2 space-y-1 break-words [overflow-wrap:anywhere]">
                <p>{adapter.message}</p>
                {#if adapter.installed_version}<p>Version: {adapter.installed_version}</p>{/if}
                {#if adapter.evidence}<p>Evidence: {adapter.evidence.replaceAll('_', ' ')}</p>{/if}
                {#if integration?.integration_active && integration.config_path}<p class="font-mono">Config: {integration.config_path}</p>{/if}
              </div>
            </details>
            {#if integration?.integration_active}
              <Button variant="outline" size="sm" disabled={removing !== null || loading || agentActivityStore.integrationsError !== null} onclick={() => removeLegacyMarker(adapter.tool_id)} title={`Remove legacy marker for ${adapter.display_name}`}>
                {removing === adapter.tool_id ? 'Removing marker…' : 'Remove legacy marker'}
              </Button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {:else if !loading && !agentActivityStore.error}
    <p class="text-meta text-muted-foreground">No observation yet. Refresh detection to check supported tools.</p>
  {/if}
  {#if actionFeedback}
    <p role={actionError ? 'alert' : 'status'} class="text-meta {actionError ? 'text-destructive' : 'text-muted-foreground'}">{actionFeedback}</p>
  {/if}
</section>
