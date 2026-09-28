<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderGit2, RefreshCw, Sparkles } from '@lucide/svelte';
  import { agentActivityStore } from '../../lib/stores/agentActivity.svelte';
  import { usageStore } from '../../lib/stores/usage.svelte';
  import Button from '../../lib/components/Button.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';
  import ProjectsPanel from '../../lib/components/ai-activity/ProjectsPanel.svelte';
  import UsagePanel from '../../lib/components/ai-activity/UsagePanel.svelte';
  import {
    AI_ACTIVITY_TAB_ORDER,
    nextAiActivityTab,
    type AiActivitySubTab,
  } from '../../lib/utils/aiActivityTabs';
  import ProjectDetailPanel from './ProjectDetailPanel.svelte';

  interface Props {
    onNavigateTab?: (tab: string) => void;
  }

  const tabLabels: Record<AiActivitySubTab, string> = {
    usage: 'Usage',
    projects: 'Projects',
  };

  let { onNavigateTab }: Props = $props();
  let activeSubTab = $state<AiActivitySubTab>('usage');
  let isMounted = $state(false);
  let selectedProject = $derived(agentActivityStore.selectedProject);
  let selectedProjectIntent = $derived(agentActivityStore.selectedProjectId !== null);
  let visibleSubTab = $derived<AiActivitySubTab>(
    selectedProject || selectedProjectIntent ? 'projects' : activeSubTab
  );
  let activeTabLabel = $derived(tabLabels[visibleSubTab]);
  let activeTabLoading = $derived(
    visibleSubTab === 'usage'
      ? usageStore.isLoading
      : agentActivityStore.isLoading
  );

  const loadedTabs = new Set<AiActivitySubTab>();
  const loadingTabs = new Set<AiActivitySubTab>();

  onMount(() => {
    isMounted = true;
    void loadTab(visibleSubTab);
    return () => {
      isMounted = false;
    };
  });

  $effect(() => {
    const tab = visibleSubTab;
    if (isMounted) void loadTab(tab);
  });

  async function loadTab(tab: AiActivitySubTab) {
    if (loadedTabs.has(tab) || loadingTabs.has(tab)) return;
    loadingTabs.add(tab);

    try {
      if (tab === 'usage') {
        await usageStore.refreshIfStale();
      } else {
        // An already observed snapshot is the agent-activity cache. Refresh only on first load.
        if (!agentActivityStore.snapshot) await agentActivityStore.refresh();
      }
      loadedTabs.add(tab);
    } finally {
      loadingTabs.delete(tab);
    }
  }

  async function handleRefreshActive() {
    const tab = visibleSubTab;
    if (tab === 'usage') {
      await usageStore.refresh(true);
    } else {
      await agentActivityStore.refresh(true);
    }
  }

  function tabId(tab: AiActivitySubTab) {
    return `ai-activity-tab-${tab}`;
  }

  function panelId(tab: AiActivitySubTab) {
    return `ai-activity-panel-${tab}`;
  }

  function selectSubTab(tab: AiActivitySubTab) {
    if (tab !== 'projects' && (selectedProject || selectedProjectIntent)) {
      agentActivityStore.selectProject(null);
    }
    activeSubTab = tab;
  }

  function handleTabKeydown(event: KeyboardEvent, currentTab: AiActivitySubTab) {
    const nextTab = nextAiActivityTab(currentTab, event.key);
    if (!nextTab) return;

    event.preventDefault();
    selectSubTab(nextTab);
    queueMicrotask(() => document.getElementById(tabId(nextTab))?.focus());
  }

  function handleBackToProjects() {
    agentActivityStore.selectProject(null);
    activeSubTab = 'projects';
  }
</script>

<div class="max-w-5xl space-y-6">
  <PageHeader
    title="AI Activity"
    subtitle="Account limits, observed agent sessions, and local workspace activity."
    icon={FolderGit2}
  >
    {#snippet actions()}
      {#if onNavigateTab}
        <Button
          variant="outline"
          size="sm"
          onclick={() => onNavigateTab('ai_control')}
          class="gap-1.5 text-xs"
        >
          <Sparkles size={13} class="text-primary" />
          <span>Control Center</span>
        </Button>
      {/if}
      <Button
        variant="outline"
        size="sm"
        disabled={activeTabLoading}
        ariaLabel={`Refresh ${activeTabLabel.toLowerCase()}`}
        title={`Refresh ${activeTabLabel.toLowerCase()}`}
        onclick={handleRefreshActive}
      >
        <RefreshCw size={13} class={activeTabLoading ? 'animate-gentle-spin' : ''} />
        {activeTabLoading ? `Refreshing ${activeTabLabel.toLowerCase()}` : `Refresh ${activeTabLabel.toLowerCase()}`}
      </Button>
    {/snippet}
  </PageHeader>

  <div
    role="tablist"
    aria-orientation="horizontal"
    aria-label="AI Activity sections"
    class="flex items-center gap-1 overflow-x-auto border-b border-border/60"
  >
    {#each AI_ACTIVITY_TAB_ORDER as tab}
      {@const isSelected = visibleSubTab === tab}
      <button
        type="button"
        id={tabId(tab)}
        role="tab"
        aria-selected={isSelected}
        aria-controls={panelId(tab)}
        tabindex={isSelected ? 0 : -1}
        class="relative shrink-0 px-3 py-2.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring {isSelected
          ? 'font-semibold text-foreground after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:bg-primary'
          : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => selectSubTab(tab)}
        onkeydown={(event) => handleTabKeydown(event, tab)}
      >
        {tabLabels[tab]}
      </button>
    {/each}
  </div>

  {#if selectedProject}
    <div
      id="ai-activity-panel-projects"
      role="tabpanel"
      aria-labelledby="ai-activity-tab-projects"
      tabindex="0"
      class="outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <ProjectDetailPanel
        project={selectedProject}
        onBack={handleBackToProjects}
        onNavigateTab={onNavigateTab}
      />
    </div>
  {:else if visibleSubTab === 'usage'}
    <UsagePanel />
  {:else}
    <ProjectsPanel />
  {/if}

  {#if onNavigateTab}
    <p class="text-meta text-muted-foreground">
      Missing a tool?
      <button type="button" class="rounded-sm text-primary underline underline-offset-2 focus-visible:ring-2 focus-visible:ring-ring" onclick={() => onNavigateTab('settings')}>
        Check tool detection in Settings → Diagnostics.
      </button>
    </p>
  {/if}
</div>
