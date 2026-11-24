<script lang="ts">
  import NeatiWordmark from '../../lib/components/NeatiWordmark.svelte';
  import { onMount } from 'svelte';
  import { navigationInbox } from '../../lib/utils/navigationInbox';
  import { fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { prefersReducedMotion } from 'svelte/motion';
  import type { CategoryResult, DashboardRoute, DashboardTab, PlatformCapabilities } from '../../lib/models/types';
  import { scanStore } from '../../lib/stores/scan.svelte';
  import { awakeStore } from '../../lib/stores/awake.svelte';
  import { settingsStore } from '../../lib/stores/settings.svelte';
  import { updateStore } from '../../lib/stores/updates.svelte';
  import OnboardingDialog from '../../lib/components/OnboardingDialog.svelte';
  import { platformCapabilitiesStore } from '../../lib/stores/platformCapabilities.svelte';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import PreviewModeIndicator from '../../lib/components/PreviewModeIndicator.svelte';
  import StorageView from './StorageView.svelte';
  import CategoryDetailView from './CategoryDetailView.svelte';
  import DockerView from './DockerView.svelte';
  import ModelsView from './ModelsView.svelte';
  import PerformanceView from './PerformanceView.svelte';
  import DevelopmentServersView from './DevelopmentServersView.svelte';
  import ProjectCockpitView from './ProjectCockpitView.svelte';
  import AiControlCenterView from './AiControlCenterView.svelte';
  import AgentSessionsView from './AgentSessionsView.svelte';
  import OverviewView from './OverviewView.svelte';
  import AwakeView from './AwakeView.svelte';
  import SettingsView from './SettingsView.svelte';
  import { APP_VERSION, formatVersion } from '../../lib/utils/version';
  import { formatBytes } from '../../lib/utils/format';
  import { SECTIONS, sectionForRoute, sectionTabForRoute, sectionEntryRoute, dashboardNavigationOwner, dashboardStage, BETA_NOTE, initialDashboardRoute, normalizeDashboardTab, visibleSectionRoutes, moreSectionRoutes, type SectionDef, type SectionId, type SectionRoute } from '../../lib/utils/dashboardNavigation';
  import SectionMoreMenu from '../../lib/components/SectionMoreMenu.svelte';
  import SegmentedTabs from '../../lib/components/SegmentedTabs.svelte';
  import { isTauri, tauriStartWindowDrag, tauriTakePendingNavigation, observeDashboardNavigation } from '../../lib/utils/tauri';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import Button from '../../lib/components/Button.svelte';
  import BrandIcon from '../../lib/components/BrandIcon.svelte';
  import Card from '../../lib/components/Card.svelte';
  import {
    AlertCircle,
    Brush,
    ChevronsLeft,
    ChevronsRight,
    House,
    RotateCw,
    Settings,
    Shield,
    Sparkles,
  } from '@lucide/svelte';

  /** Routes the shell can show: persisted routes plus the storage sub-views and
   * the Performance local tabs the Overview links into. */
  type Tab =
    | DashboardRoute
    | 'large-files'
    | 'applications'
    | 'developer-artifacts'
    | 'disks'
    | 'cpu'
    | 'battery'
    | 'sessions';

  let currentTab = $state<Tab>('storage');
  let selectedCategory = $state<CategoryResult | null>(null);
  // SSR has no side effects, so render the existing dashboard shape for
  // snapshot tests. Browser mounts wait for the backend capability response
  // before mounting any platform-sensitive child route.
  let capabilitiesReady = $state(typeof window === 'undefined');
  // Derived from the store rather than a local flag so a failed query and an
  // unsupported platform can never be confused with each other.
  let capabilitiesFailed = $derived(
    platformCapabilitiesStore.error !== null && platformCapabilitiesStore.capabilities === null
  );
  let disposed = false;
  let workflowsMounted = false;
  let stopFreshness: (() => void) | undefined;
  let settings = $derived(settingsStore.settings);
  let sidebarCollapsed = $derived(settings.sidebar_collapsed ?? false);
  // Only the platform that actually draws an overlay title bar reserves the top
  // band or gets a drag strip; a native caption bar would otherwise show dead
  // pixels below it.
  let overlayTitleBar = $derived(platformContextStore.overlayTitleBar);
  let fadeDuration = $derived(prefersReducedMotion.current ? 0 : 140);

  type DashboardCapability = keyof Omit<PlatformCapabilities, 'platform'>;

  const sectionIcons: Record<SectionId, typeof House> = {
    home: House,
    clean: Brush,
    ai: Sparkles,
    settings: Settings,
  };
  /** Sidebar entries; Settings keeps its own place at the bottom. */
  const mainSections = SECTIONS.filter((section) => section.id !== 'settings');
  /** The route each section last showed, so returning to it resumes there. */
  let lastRouteBySection = $state<Partial<Record<SectionId, string>>>({});
  /** The scrolling content column; a new view always starts at its top. */
  let contentColumn = $state<HTMLElement>();
  // Runs after the new view is in the DOM, so the old view's scroll offset is
  // never applied to it.
  $effect(() => {
    void currentTab;
    void selectedCategory;
    contentColumn?.scrollTo({ top: 0 });
  });
  let currentSection = $derived(sectionForRoute(currentTab));
  let currentSectionTab = $derived(sectionTabForRoute(currentTab));
  // A focused detail (Memory opened on its own, a cache category) hides the
  // section tabs so the page keeps one selector, as before.
  let showSectionTabs = $derived(
    currentSection.routes.length > 1 && !selectedCategory && currentTab !== 'memory'
  );
  function sectionTabItem(entry: SectionRoute) {
    return {
      id: entry.route,
      label: entry.label,
      badge: dashboardStage(entry.route) === 'beta' ? 'Beta' : undefined,
      disabled: !isRouteAvailable(entry.route),
      title: routeUnavailableReason(entry.route) ?? (dashboardStage(entry.route) === 'beta' ? BETA_NOTE : undefined),
    };
  }
  // At most two primary tabs; the rest wait in one More menu (#519).
  let sectionTabs = $derived(visibleSectionRoutes(currentSection, currentSectionTab).map(sectionTabItem));
  let sectionMoreItems = $derived(moreSectionRoutes(currentSection).map(sectionTabItem));

  /**
   * One capability per destination. A route whose adapter is missing stays
   * closed, and `memory` shares the Performance page's adapter because it is
   * that page's Memory detail.
   */
  const routeCapabilities: Record<string, DashboardCapability | null> = {
    overview: null,
    settings: null,
    storage: 'cleanup',
    'large-files': 'cleanup',
    applications: 'cleanup',
    'developer-artifacts': 'developer_artifacts',
    disks: 'cleanup',
    performance: 'memory_metrics',
    memory: 'memory_metrics',
    cpu: 'cpu_metrics',
    battery: 'battery_metrics',
    docker: 'docker',
    models: 'local_models',
    development_servers: 'development_ports',
    projects: 'ai_integrations',
    usage: 'ai_integrations',
    sessions: 'ai_integrations',
    ai_control: 'ai_integrations',
    awake: 'keep_awake',
  };

  /** Whether a destination can run on this platform right now. */
  function isRouteAvailable(route: string): boolean {
    if (!capabilitiesReady || capabilitiesFailed) return false;
    if (route === 'applications' && !platformCapabilitiesStore.isInspectable('installed_apps')) return false;
    const capability = routeCapabilities[normalizeDashboardTab(route)] ?? null;
    return !capability || platformCapabilitiesStore.isAvailable(capability);
  }

  /** Why a route is closed on this platform, when it is. */
  function routeUnavailableReason(route: string): string | undefined {
    if (isRouteAvailable(route)) return undefined;
    const capabilityName = route === 'applications' ? 'installed_apps' : routeCapabilities[normalizeDashboardTab(route)];
    const reason = capabilityName ? platformCapabilitiesStore.feature(capabilityName)?.reason : undefined;
    return reason ?? 'Unavailable on this platform';
  }

  function isSectionAvailable(section: SectionDef): boolean {
    return section.routes.some((entry) => isRouteAvailable(entry.route));
  }

  function selectSection(section: SectionDef) {
    const route = sectionEntryRoute(section, lastRouteBySection[section.id], isRouteAvailable);
    if (route) selectTab(route);
  }

  /**
   * Loads the backend capability matrix and, once it is known, mounts the
   * platform-sensitive workflows. Retrying after a failure repeats the same
   * path, so a transient IPC error cannot leave the dashboard permanently
   * gated and identical to an unsupported platform.
   */
  async function refreshCapabilities(force: boolean) {
    await platformCapabilitiesStore.load(force);
    if (disposed) return;

    capabilitiesReady = true;
    if (capabilitiesFailed || workflowsMounted) return;
    workflowsMounted = true;

    const cleanupAvailable = platformCapabilitiesStore.isAvailable('cleanup');
    const awakeAvailable = platformCapabilitiesStore.isAvailable('keep_awake');

    // Do not mount or invoke platform-sensitive workflows until the backend
    // has told us that the corresponding adapter is available.
    if (cleanupAvailable) {
      stopFreshness = scanStore.observeFreshness();
      // Show the cached scan immediately; the freshness timer (#128)
      // auto-rescans within ~1s if it is stale, so no deferred scan here.
      void scanStore.init();
    }
    if (awakeAvailable) void awakeStore.refresh();

    // A persisted tab may have become unavailable after an upgrade or on a
    // different platform. Start on the first available tab instead of
    // mounting an unsupported route.
    currentTab = initialDashboardRoute(isRouteAvailable) as Tab;
  }

  onMount(() => {
    let stopNavigation: (() => void) | undefined;
    const navigation = navigationInbox(tauriTakePendingNavigation, selectTab);
    void platformContextStore.load();
    void (async () => {
      if (isTauri()) {
        const stop = await observeDashboardNavigation(() => { void navigation.wake(); });
        if (disposed) { stop(); return; }
        stopNavigation = stop;
      }
      await refreshCapabilities(false);
      // The opt-in launch update check runs once per main-window lifetime.
      await settingsStore.load();
      if (!disposed) void updateStore.checkOnLaunch(settingsStore.settings.check_updates_on_launch);
      // Subscribe before the initial drain: no mount/listen race, and warm
      // windows consume later requests even when already focused.
      if (isTauri()) await navigation.activate();
    })();

    return () => {
      disposed = true;
      navigation.dispose();
      stopNavigation?.();
      stopFreshness?.();
    };
  });

  function selectTab(tab: Tab | string) {
    if (!isRouteAvailable(tab)) return;

    // The former `memory` route is the Memory detail of the Performance page,
    // so it keeps its own destination instead of collapsing into the default
    // CPU detail.
    if (tab === 'memory') {
      currentTab = 'memory';
      selectedCategory = null;
      return;
    }

    tab = normalizeDashboardTab(tab);
    if (tab === 'developer_artifacts' || tab === 'developer-artifacts') {
      currentTab = 'developer-artifacts';
    } else if (tab === 'large_files' || tab === 'large-files') {
      currentTab = 'large-files';
    } else if (tab === 'applications') {
      currentTab = 'applications';
    } else if (tab === 'disks') {
      currentTab = 'disks';
    } else if (tab === 'cpu' || tab === 'battery') {
      currentTab = tab;
    } else if (tab === 'settings') {
      currentTab = 'settings';
    } else {
      currentTab = tab as Tab;
    }
    selectedCategory = null;
    lastRouteBySection[sectionForRoute(currentTab).id] = currentTab;
  }

  async function toggleSidebar() {
    await settingsStore.save({ sidebar_collapsed: !settingsStore.settings.sidebar_collapsed });
  }

  function handleWindowDrag(event: MouseEvent) {
    if (event.button !== 0) return;
    const target = event.target;
    if (target instanceof Element && target.closest('.no-drag')) return;
    void tauriStartWindowDrag().catch(() => undefined);
  }
</script>

<div class="dashboard-shell flex h-screen w-full bg-background text-foreground overflow-hidden font-sans select-none relative">
  {#if overlayTitleBar}
    <!-- Window drag region for the macOS overlay title bar -->
    <div
      class="titlebar-drag-region absolute top-0 left-0 right-0 h-7 z-30"
      aria-hidden="true"
      onmousedown={handleWindowDrag}
    ></div>
  {/if}
  <!-- Sidebar Navigation -->
  <aside
    class="liquid-sidebar {sidebarCollapsed ? 'w-16 p-2' : 'w-56 p-3'} shrink-0 border-r border-border flex flex-col justify-between {overlayTitleBar
      ? 'pt-9'
      : ''} relative transition-[width,padding] duration-fast"
  >
    <div class="shrink-0 pb-4 flex items-center {sidebarCollapsed ? 'flex-col' : 'justify-between'} gap-2">
        <div
          class="{sidebarCollapsed ? 'px-0' : 'px-2.5'} flex items-center space-x-2.5 {overlayTitleBar
            ? 'titlebar-drag-region'
            : ''}"
          role="presentation"
          onmousedown={overlayTitleBar ? handleWindowDrag : undefined}
        >
          <BrandIcon identity="neati" label="neati" size={24} />
          {#if !sidebarCollapsed}
            <NeatiWordmark />
          {/if}
        </div>

        <Button
          variant="ghost"
          size="icon"
          class="no-drag h-7 w-7 shrink-0 rounded-md border border-transparent text-muted-foreground hover:border-border hover:bg-card hover:text-foreground"
          aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          title={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          onclick={toggleSidebar}
        >
          {#if sidebarCollapsed}
            <ChevronsRight size={15} strokeWidth={1.8} />
          {:else}
            <ChevronsLeft size={15} strokeWidth={1.8} />
          {/if}
        </Button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto scroll-stable">
      <!-- Navigation Links -->
      <nav class="space-y-0.5 no-drag" aria-label="Main Navigation">
        {#each mainSections as section (section.id)}
          {@const Icon = sectionIcons[section.id]}
          {@const available = isSectionAvailable(section)}
          {@const active = currentSection.id === section.id}
          {@const reclaimable = section.id === 'clean' ? scanStore.reclaimableBytes : 0}
          <button
            type="button"
            onclick={() => selectSection(section)}
            disabled={!available}
            aria-current={active ? 'page' : undefined}
            aria-label={reclaimable > 0 ? `${section.label}, ${formatBytes(reclaimable)} reclaimable` : section.label}
            title={available ? (sidebarCollapsed ? section.label : undefined) : `${section.label} is unavailable on this platform`}
            data-section={section.id}
            class="relative w-full flex items-center {sidebarCollapsed ? 'justify-center px-0' : 'gap-2.5 px-2.5'} py-2 rounded-md text-body font-medium transition-[background-color,color] duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {active
              ? 'bg-accent text-primary font-semibold'
              : available
                ? 'text-muted-foreground hover:text-foreground hover:bg-card'
                : 'text-muted-foreground/50 cursor-not-allowed'}"
          >
            {#if active}<span aria-hidden="true" class="absolute left-0 inset-y-2 w-0.5 rounded-full bg-primary"></span>{/if}
            <Icon size={17} strokeWidth={1.75} class="shrink-0" />
            {#if !sidebarCollapsed}
              <span class="truncate">{section.label}</span>
            {/if}
            {#if reclaimable > 0}
              <span
                class="{sidebarCollapsed
                  ? 'absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-primary'
                  : 'ml-auto inline-flex items-center gap-1.5 whitespace-nowrap text-caption font-medium tabular-nums text-primary'}"
                title="Reclaimable storage available"
              >
                {#if sidebarCollapsed}
                  <span class="sr-only">{formatBytes(reclaimable)} reclaimable</span>
                {:else}
                  <span aria-hidden="true" class="h-1.5 w-1.5 rounded-full bg-primary/90"></span>
                  {formatBytes(reclaimable)}
                {/if}
              </span>
            {/if}
          </button>
        {/each}
      </nav>
    </div>

    <!-- Settings stays reachable above the footer when the navigation scrolls. -->
    <div class="shrink-0 space-y-2 {sidebarCollapsed ? 'items-center' : ''}">
      <div class="border-t border-border/70 pt-2">
        {#if !sidebarCollapsed}
          <div class="px-2.5 pb-1 text-caption font-medium tracking-normal text-muted-foreground select-none">
            Preferences
          </div>
        {/if}
        <button
          type="button"
          onclick={() => selectTab('settings')}
          aria-current={currentTab === 'settings' ? 'page' : undefined}
          aria-label="Settings"
          title={sidebarCollapsed ? 'Settings' : undefined}
          class="relative w-full flex items-center {sidebarCollapsed ? 'justify-center px-0' : 'gap-2.5 px-2.5'} py-2 rounded-md text-body font-medium transition-[background-color,color] duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {currentTab ===
          'settings'
            ? 'bg-accent text-primary font-semibold'
            : 'text-muted-foreground hover:text-foreground hover:bg-card'}"
        >
          {#if currentTab === 'settings'}<span aria-hidden="true" class="absolute left-0 inset-y-2 w-0.5 rounded-full bg-primary"></span>{/if}
          <Settings size={17} strokeWidth={1.75} class="shrink-0" />
          {#if !sidebarCollapsed}
            <span>Settings</span>
          {/if}
        </button>
      </div>
      <div
        class="{sidebarCollapsed ? 'justify-center px-0' : 'px-2.5'} py-2 rounded-lg bg-card/70 border border-border text-meta text-muted-foreground flex items-center gap-2"
        title="Path, symlink and filesystem identity are re-derived immediately before deletion, and a refused item is reported instead of being removed."
      >
        <Shield size={14} class="text-muted-foreground shrink-0" />
        {#if !sidebarCollapsed}
          <span class="truncate">Verified on delete</span>
        {/if}
      </div>
      <PreviewModeIndicator compact={sidebarCollapsed} />
      {#if !sidebarCollapsed}
        <div class="px-2.5 flex items-center justify-between text-caption text-muted-foreground font-mono select-none">
          <span>neati</span>
          <span>{formatVersion(APP_VERSION)}</span>
        </div>
      {/if}
    </div>
  </aside>

  <!-- Main Content Area with fluid native transition -->
  <main bind:this={contentColumn} class="@container min-w-0 flex-1 h-full overflow-y-auto scroll-stable scroll-pb-24 {overlayTitleBar ? 'pt-10' : ''}">
    {#if !capabilitiesReady}
      <div class="flex h-full items-center justify-center text-body text-muted-foreground p-4 @2xl:p-6">Loading platform capabilities…</div>
    {:else if capabilitiesFailed}
      <div class="flex h-full items-center justify-center p-4 @2xl:p-6">
        <Card class="max-w-md p-6 space-y-3 text-center">
          <div class="mx-auto h-9 w-9 rounded-lg bg-destructive/10 text-destructive flex items-center justify-center">
            <AlertCircle size={18} />
          </div>
          <div class="space-y-1">
            <h2 class="text-sm font-semibold text-foreground">Platform capabilities unavailable</h2>
            <p class="text-body text-muted-foreground break-words">
              {platformCapabilitiesStore.error ?? 'neati could not read this platform\'s capability matrix from the backend.'}
            </p>
            <p class="text-meta text-muted-foreground">
              Tabs stay closed until the backend answers so no native action runs on an unverified platform.
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={platformCapabilitiesStore.isLoading}
            onclick={() => void refreshCapabilities(true)}
            class="mx-auto gap-1.5"
          >
            <LoadingActionContent busy={platformCapabilitiesStore.isLoading} busyLabel="Retrying…">
              <RotateCw size={14} aria-hidden="true" /><span>Retry</span>
            </LoadingActionContent>
          </Button>
        </Card>
      </div>
    {:else}
      <div class="p-4 @2xl:p-6">
        {#if showSectionTabs}
          <div class="mb-4 flex items-end gap-2">
            <SegmentedTabs
              tabs={sectionTabs}
              activeTab={currentSectionTab}
              onSelect={(route) => selectTab(route)}
              ariaLabel={`${currentSection.label} views`}
              appearance="underline"
              class="min-w-0 flex-1"
            />
            {#if sectionMoreItems.length > 0}
              <SectionMoreMenu
                items={sectionMoreItems}
                activeId={currentSectionTab}
                sectionLabel={currentSection.label}
                onSelect={(route) => selectTab(route)}
              />
            {/if}
          </div>
        {/if}
        {#key selectedCategory ? selectedCategory.category : dashboardNavigationOwner(currentTab) === 'storage' ? 'storage' : currentTab}
          <div in:fade={{ duration: fadeDuration, easing: cubicOut }}>
            {#if selectedCategory}
              <CategoryDetailView
                categoryResult={scanStore.lastScan?.categories.find((category) => category.category === selectedCategory?.category) ?? { ...selectedCategory, items: [], total_bytes: 0, safe_bytes: 0, rebuild_bytes: 0, manual_bytes: 0 }}
                onBack={() => (selectedCategory = null)}
                onNavigateTab={(tab) => selectTab(tab)}
              />
            {:else if currentTab === 'overview'}
              <OverviewView onNavigateTab={(tab) => selectTab(tab)} />
            {:else if currentTab === 'storage' || currentTab === 'large-files' || currentTab === 'applications' || currentTab === 'developer-artifacts' || currentTab === 'disks'}
              <StorageView
                onNavigateTab={selectTab}
                initialTab={currentTab === 'storage' ? 'cleanup' : currentTab}
                onSelectWorkflow={(tab) => selectTab(tab === 'cleanup' ? 'storage' : tab)}
                onSelectCategory={(cat) => (selectedCategory = cat)}
              />
            {:else if currentTab === 'performance' || currentTab === 'memory' || currentTab === 'cpu' || currentTab === 'battery'}
              <PerformanceView
                standaloneMemory={currentTab === 'memory'}
                initialTab={currentTab === 'memory' ? 'memory' : currentTab === 'battery' ? 'battery' : 'cpu'}
                onNavigateTab={(tab) => selectTab(tab)}
              />
            {:else if currentTab === 'docker'}
              <DockerView />
            {:else if currentTab === 'models'}
              <ModelsView />
            {:else if currentTab === 'projects' || currentTab === 'usage'}
              <ProjectCockpitView onNavigateTab={(tab) => selectTab(tab)} />
            {:else if currentTab === 'sessions'}
              <AgentSessionsView />
            {:else if currentTab === 'ai_control'}
              <AiControlCenterView onNavigateTab={(tab) => selectTab(tab)} />
            {:else if currentTab === 'development_servers'}
              <DevelopmentServersView />
            {:else if currentTab === 'awake'}
              <AwakeView />
            {:else if currentTab === 'settings'}
              <SettingsView onCategory={(category) => { selectTab('storage'); selectedCategory = category; }} onNavigate={selectTab} />
            {/if}
          </div>
        {/key}
      </div>
    {/if}
  </main>
</div>

<!-- First-run guide. The store defaults to completed until settings load, so
     it never flashes for someone who already finished it. -->
{#if !settings.onboarding_completed}
  <OnboardingDialog onFinish={() => void settingsStore.save({ onboarding_completed: true })} />
{/if}
