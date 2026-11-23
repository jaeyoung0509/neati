<script lang="ts">
  import { onMount } from 'svelte';
  import AiUsageCards from '../AiUsageCards.svelte';
  import Card from '../Card.svelte';
  import LoadingState from '../LoadingState.svelte';
  import { usageStore } from '../../stores/usage.svelte';
  import { awaitingFirstAnswer } from '../../utils/loadPhase';

  // Revalidate the TTL cache while this tab stays open; hidden panels never poll.
  onMount(() => usageStore.observeAutoRefresh());

  let usageSnapshot = $derived(usageStore.snapshot);
  let providers = $derived(usageStore.providers);
  let connectedCount = $derived(providers.filter((provider) => provider.connected).length);
</script>

<div
  id="ai-activity-panel-usage"
  role="tabpanel"
  aria-labelledby="ai-activity-tab-usage"
  tabindex="0"
  class="space-y-4 outline-none focus-visible:ring-1 focus-visible:ring-ring"
>
  <div class="flex items-center justify-between gap-3">
    <div>
      <h3 class="text-sm font-semibold">Accounts &amp; usage</h3>
      <p class="mt-1 text-meta text-muted-foreground">
        Usage from supported clients, with local activity labeled separately.
      </p>
    </div>
    {#if usageSnapshot}
      <span class="shrink-0 text-caption text-muted-foreground">{connectedCount} connected</span>
    {/if}
  </div>

  {#if usageStore.error}
    <div role="alert" class="rounded-xl border status-panel-destructive px-4 py-3 text-meta text-destructive">
      {usageSnapshot ? 'Usage refresh failed. The last successful usage snapshot remains visible.' : 'Usage could not be loaded.'} {usageStore.error}
    </div>
  {/if}

  {#if providers.length > 0}
    <AiUsageCards
      providers={providers}
      isProviderLoading={(id) => usageStore.isProviderLoading(id)}
      openRouterAction={usageStore.openRouterAction}
      onConnectOpenRouter={() => usageStore.connectOpenRouter()}
      onDisconnectOpenRouter={() => usageStore.disconnectOpenRouter()}
    />
  {:else if usageStore.isLoading || awaitingFirstAnswer(usageStore.loadPhase)}
    <!-- Before the first snapshot answers, no providers is not a result (#491). -->
    <LoadingState message="Checking account usage…" />
  {:else if usageStore.loadPhase === 'ready'}
    <Card class="p-4 text-meta text-muted-foreground">
      No AI account usage metadata is available yet.
    </Card>
  {/if}
</div>
