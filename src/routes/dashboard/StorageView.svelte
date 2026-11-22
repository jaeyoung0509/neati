<script lang="ts">
  import EmptyTrashAction from "../../lib/components/EmptyTrashAction.svelte";
  import CleanupQuitDialog from '../../lib/components/CleanupQuitDialog.svelte';
  import { quitAndRescan } from '../../lib/utils/cleanupQuit';
  import CleanupReviewDialog from '../../lib/components/CleanupReviewDialog.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import ScanFreshnessNotice from '../../lib/components/ScanFreshnessNotice.svelte';
  import { onMount, tick } from 'svelte';
  import type { CategoryResult, DashboardTab, PlanPreview, CleanupQuitPreview } from '../../lib/models/types';
  import { scanStore } from '../../lib/stores/scan.svelte';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import { platformCapabilitiesStore } from '../../lib/stores/platformCapabilities.svelte';
  import {
    tauriOpenStorageSettings, tauriPreviewCleanupQuit, tauriTerminateMemoryGroup,
  } from '../../lib/utils/tauri';
  import Button from '../../lib/components/Button.svelte';
  import Card from '../../lib/components/Card.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';
  import ProgressBar from '../../lib/components/ProgressBar.svelte';
  import CategoryCard from '../../lib/components/CategoryCard.svelte';
  import Checkbox from '../../lib/components/Checkbox.svelte';
  import StorageSummary from '../../lib/components/StorageSummary.svelte';
  import StorageScanProgress from '../../lib/components/StorageScanProgress.svelte';
  import StorageScanDetails from '../../lib/components/StorageScanDetails.svelte';
  import CleanupGuidanceList from '../../lib/components/CleanupGuidanceList.svelte';
  import EmptyState from '../../lib/components/EmptyState.svelte';
  import { cleanupView } from '../../lib/utils/cleanupView';
  import CleanResultModal from '../../lib/components/CleanResultModal.svelte';
  import LoadingIndicator from '../../lib/components/LoadingIndicator.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import SelectionToolbar from '../../lib/components/SelectionToolbar.svelte';
  import ProjectBuildOutputCard from '../../lib/components/ProjectBuildOutputCard.svelte';
  import { projectCleanupStore } from '../../lib/stores/projectCleanup.svelte';
  import { formatBytes } from '../../lib/utils/format';
  import DeveloperArtifactsView from './DeveloperArtifactsView.svelte';
  import LargeFilesView from './LargeFilesView.svelte';
  import ApplicationsView from './ApplicationsView.svelte';
  import DiskView from './DiskView.svelte';
  import { isActionable, summarizeCategory } from '../../lib/utils/cleanup';
  import {
    RotateCw,
    HardDrive,
    ExternalLink,
    Square,
  } from '@lucide/svelte';

  interface Props {
    onSelectCategory: (category: CategoryResult) => void;
    onNavigateTab?: (tab: DashboardTab) => void;
    onOpenLargeFiles?: () => void;
    onOpenApplications?: () => void;
    onOpenDeveloperArtifacts?: () => void;
    onOpenDisks?: () => void;
    initialTab?: 'cleanup' | 'developer-artifacts' | 'large-files' | 'applications' | 'disks';
    onSelectWorkflow?: (tab: 'cleanup' | 'developer-artifacts' | 'large-files' | 'applications' | 'disks') => void;
  }

  let {
    onSelectCategory,
    onNavigateTab,
    onOpenLargeFiles,
    onOpenApplications,
    onOpenDeveloperArtifacts,
    onOpenDisks,
    initialTab = 'cleanup',
    onSelectWorkflow,
  }: Props = $props();

  let activeSecondaryTab = $state<'cleanup' | 'developer-artifacts' | 'large-files' | 'applications' | 'disks'>('cleanup');

  onMount(() => {
    void platformCapabilitiesStore.load();
    void platformContextStore.load();
    // Project build output is part of Clean (#494); scan saved folders once.
    if (projectCleanupStore.phase === 'idle') void projectCleanupStore.refresh();
    return () => { quitCancelled = true; };
  });

  let isApplicationsInspectable = $derived(
    platformCapabilitiesStore.isInspectable('installed_apps')
  );

  $effect(() => {
    if (initialTab === 'applications' && !isApplicationsInspectable) {
      activeSecondaryTab = 'cleanup';
    } else {
      activeSecondaryTab = initialTab;
    }
  });
  let scan = $derived(scanStore.lastScan);
  let presentation = $derived(cleanupView(scan, scanStore.freshness, scanStore.discovery, !!scanStore.error));
  let orderedCategories = $derived(
    [...(scan?.categories ?? [])].sort((a, b) =>
      summarizeCategory(b.items).cleanable_bytes - summarizeCategory(a.items).cleanable_bytes
    )
  );
  let hasSelectedAction = $derived(
    scan?.categories.some(category => category.items.some(
      item => scanStore.selectedMap[item.id] && isActionable(item)
    )) ?? false
  );
  let projectSelection = $derived(projectCleanupStore.selectedItems);
  let projectNotice = $state<string | null>(null);

  function rescanAll() {
    void scanStore.runScan();
    void projectCleanupStore.refresh();
  }

  /** The one Clean action also removes the selected project build output (#494). */
  async function cleanProjectBuildOutput() {
    if (projectCleanupStore.selectedIds.length === 0) return;
    const removal = await projectCleanupStore.clean();
    if (!removal) {
      projectNotice = projectCleanupStore.error;
      return;
    }
    const kept = removal.failed_count + removal.skipped_count;
    projectNotice = `Removed ${formatBytes(removal.removed_allocated_size)} of project build output`
      + (kept > 0 ? ` · ${kept} kept (in use or changed)` : '') + '.';
  }
  let showResultModal = $state(false);
  let review = $state<{ scanId: string; plan: PlanPreview } | null>(null);
  let isPreparingReview = $state(false);
  let quitPreview = $state<CleanupQuitPreview | null>(null);
  let quitBusy = $state(false);
  let quitProgress = $state('');
  let quitCancelled = false;
  let itemDetailsOpen = $state(false);
  let runningItems = $derived(scan?.categories.flatMap(category => category.items)
    .filter(item => item.owner_running && isActionable(item)) ?? []);

  let isPreviewingQuit = $state(false);

  async function previewQuit() {
    // Both the summary and the guidance list can ask; one review at a time.
    if (!scan || !scanStore.canClean || isPreparingReview) return;
    isPreparingReview = true;
    isPreviewingQuit = true;
    try {
      quitProgress = '';
      quitPreview = await tauriPreviewCleanupQuit(scan.scan_id, runningItems.map(item => item.id));
    } catch (error) { scanStore.error = String(error); }
    finally { isPreparingReview = false; isPreviewingQuit = false; }
  }

  function cancelQuit() {
    quitCancelled = true;
    if (!quitBusy) quitPreview = null;
  }

  async function confirmQuit() {
    if (!quitPreview || quitPreview.scan_id !== scan?.scan_id || !scanStore.canClean) {
      quitProgress = 'The scan changed. Close this dialog and try again.';
      return;
    }
    quitBusy = true;
    quitCancelled = false;
    try {
      const items = await quitAndRescan(quitPreview, {
        quit: lease => tauriTerminateMemoryGroup(lease, 'graceful'),
        scan: () => scanStore.runScan(),
        cancelled: () => quitCancelled,
        progress: message => { quitProgress = message; },
      });
      if (quitCancelled) { quitPreview = null; return; }
      if (!items.length) { quitProgress = 'No reviewed caches are ready. Check the latest scan.'; return; }
      const scanId = scanStore.lastScan?.scan_id;
      // Provider units can still require review after their owner exits and
      // therefore are not preselected by the fresh scan.
      for (const item of items) scanStore.setItemSelected(item.id, true);
      const plan = await scanStore.prepareCleanup(items);
      if (quitCancelled) { quitPreview = null; return; }
      if (plan && scanId && scanStore.lastScan?.scan_id === scanId && scanStore.canClean) {
        quitPreview = null;
        // Always review newly available targets after a quit; the original
        // dialog authorized app shutdown, not an unseen deletion plan.
        review = { scanId, plan };
      } else { quitProgress = scanStore.error ?? 'The caches changed. Close this dialog and check the latest scan.'; }
    } catch (error) { quitProgress = String(error); }
    finally { quitBusy = false; }
  }

  const storagePanelId = $props.id();
  function openItemDetails() {
    itemDetailsOpen = true;
    void tick().then(() => {
      const control = document.getElementById(`${storagePanelId}-item-details`);
      control?.scrollIntoView({ block: 'center' });
      control?.focus({ preventScroll: true });
    });
  }
  const baseStorageTabs = [
    { id: 'cleanup', label: 'Cleanup' },
    { id: 'developer-artifacts', label: 'Developer Artifacts' },
    { id: 'large-files', label: 'Large Files' },
    { id: 'applications', label: 'Applications' },
    { id: 'disks', label: 'Disks' },
  ];
  let storageTabs = $derived(
    baseStorageTabs.filter((tab) => tab.id !== 'applications' || isApplicationsInspectable)
  );

  async function handleCleanSelected() {
    if (!scan || !scanStore.canClean) return;
    projectNotice = null;
    if (!hasSelectedAction) {
      await cleanProjectBuildOutput();
      return;
    }
    const scanId = scan.scan_id;
    const items = scan.categories.flatMap(category => category.items)
      .filter(item => scanStore.selectedMap[item.id] && isActionable(item));
    isPreparingReview = true;
    try {
      const plan = await scanStore.prepareCleanup(items);
      if (plan && scanStore.lastScan?.scan_id === scanId && scanStore.canClean) {
        if (plan.requires_confirmation) {
          review = { scanId, plan };
        } else {
          const result = await scanStore.executePreparedPlan(plan, false);
          if (result) showResultModal = true;
          await cleanProjectBuildOutput();
        }
      }
    } finally {
      isPreparingReview = false;
    }
  }

  function confirmCleanup() {
    if (!review || review.scanId !== scan?.scan_id || !scanStore.canClean) return;
    const plan = review.plan;
    review = null;
    // The review trigger becomes disabled during cleanup. Focus the active
    // panel itself while work runs; restoring to the tab made WebKit draw a
    // prominent blue focus rectangle around a tab the user had not selected.
    queueMicrotask(() => document.getElementById(storagePanelId)?.focus());
    // Execute only the reviewed selection, even if another consumer selected
    // additional items while the review was open. Backend plans revalidate it.
    scanStore.executePreparedPlan(plan, true).then((res) => {
      if (res) {
        showResultModal = true;
        void cleanProjectBuildOutput();
      } else {
        document.getElementById(storagePanelId)?.focus();
      }
    });
  }

  function handleTabClick(tab: 'cleanup' | 'developer-artifacts' | 'large-files' | 'applications' | 'disks') {
    if (tab === 'applications' && !isApplicationsInspectable) {
      return;
    }
    if (onSelectWorkflow) {
      onSelectWorkflow(tab);
      void tick().then(() => document.getElementById(storagePanelId)?.focus());
      return;
    }
    if (tab === 'developer-artifacts' && onOpenDeveloperArtifacts) {
      onOpenDeveloperArtifacts();
      return;
    }
    if (tab === 'large-files' && onOpenLargeFiles) {
      onOpenLargeFiles();
      return;
    }
    if (tab === 'applications' && onOpenApplications) {
      onOpenApplications();
      return;
    }
    if (tab === 'disks' && onOpenDisks) {
      onOpenDisks();
      return;
    }
    activeSecondaryTab = tab;
    void tick().then(() => document.getElementById(storagePanelId)?.focus());
  }
</script>


<div class="storage-workspace space-y-4">
  {#if activeSecondaryTab === 'cleanup'}
  <PageHeader title="Caches" icon={HardDrive} class="storage-header">
    {#snippet actions()}
      {#if platformContextStore.context?.platform === "macos"}
        <EmptyTrashAction disabled={scanStore.isScanning || scanStore.isCleaning} />
      {/if}
      <Button
        variant="outline"
        size="sm"
        disabled={scanStore.isScanning || scanStore.isCleaning}
        onclick={rescanAll}
        class="gap-1.5"
        id="storage-scan-button"
        motion="paint"
      >
        <LoadingActionContent busy={scanStore.isScanning} busyLabel="Scanning…" word="scanning">
          <RotateCw size={13} aria-hidden="true" /><span>Scan Storage</span>
        </LoadingActionContent>
      </Button>
    {/snippet}
  </PageHeader>
  {/if}

  {#if platformCapabilitiesStore.error !== null && platformCapabilitiesStore.capabilities === null}
    <InlineNotice
      variant="destructive"
      message={`Could not verify installed-application inspection: ${platformCapabilitiesStore.error}. Retry to restore Applications.`}
      actionLabel="Retry"
      onAction={() => void platformCapabilitiesStore.load(true)}
    />
  {/if}

  <div
    class="space-y-4 rounded-xl outline-none"
    id={storagePanelId}
    role="region"
    aria-label={storageTabs.find(tab => tab.id === activeSecondaryTab)?.label}
    tabindex="-1"
  >
  {#if activeSecondaryTab === 'developer-artifacts'}
    <DeveloperArtifactsView onBack={() => handleTabClick('cleanup')} />
  {:else if activeSecondaryTab === 'large-files'}
    <LargeFilesView onBack={() => handleTabClick('cleanup')} />
  {:else if activeSecondaryTab === 'applications'}
    <ApplicationsView onBack={() => handleTabClick('cleanup')} />
  {:else if activeSecondaryTab === 'disks'}
    <DiskView
      onReviewCategory={(cat) => {
        activeSecondaryTab = 'cleanup';
        onSelectCategory(cat);
      }}
      onBack={() => handleTabClick('cleanup')}
    />
  {:else}
    {#if !scanStore.isScanning && !scanStore.isCleaning && !scanStore.isRefreshingAfterClean}
      <StorageSummary onQuit={platformContextStore.context?.platform === 'macos' ? previewQuit : undefined} onReview={openItemDetails} actionsDisabled={isPreparingReview || quitBusy} quitBusy={isPreviewingQuit} />
    {/if}

    <!-- No old inventory or selection is exposed while the new scan runs. -->
    {#if scanStore.isScanning}
      <StorageScanProgress />
    {/if}

    <!-- Cleaning In Progress Bar -->
    {#if scanStore.isCleaning}
      <Card class="p-4 bg-secondary/60 border-primary/40 transition-colors duration-normal">
        <div class="space-y-2" role="status" aria-live="polite">
          <div class="flex flex-wrap items-center justify-between gap-3 text-meta">
            <span class="min-w-0 font-medium text-foreground flex items-center gap-2">
              <LoadingIndicator word="cleaning" size="xs" />
              <span class="min-w-0 break-words">Cleaning: {scanStore.cleanProgress.currentItem}</span>
            </span>
            <span class="shrink-0 font-mono text-muted-foreground font-semibold">
              {scanStore.cleanProgress.index} / {scanStore.cleanProgress.total} ({scanStore.cleanProgress.percent}%)
            </span>
          </div>
          <ProgressBar value={scanStore.cleanProgress.percent} height="h-2" color="bg-primary" />
        </div>
      </Card>
    {/if}

    <!-- Error Alert -->
    {#if scanStore.error}
      <InlineNotice
        variant="destructive"
        title="Storage check needs attention"
        message={scanStore.error}
      />
    {/if}

    <!-- Scan freshness / remediation notice -->
    {#if scan && !scanStore.isScanning && !scanStore.isCleaning && !scanStore.isRefreshingAfterClean}
    <div class="space-y-2">
    {#if (scanStore.freshness !== 'fresh' && scanStore.freshness !== 'failed') || scanStore.discovery.status !== 'exhausted'}
      <ScanFreshnessNotice compact showRetry={false} />
    {/if}
      <StorageScanDetails {scan} discovery={scanStore.discovery} />
    </div>
    {/if}

    <!-- Categories Section -->
    {#if !scanStore.isScanning && !scanStore.isCleaning && !scanStore.isRefreshingAfterClean}
    <div class="space-y-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h2 class="text-sm font-semibold text-foreground">Caches</h2>
        {#if scan}
          <span class="text-meta text-muted-foreground">Largest cleanup first</span>
        {/if}
      </div>

      {#if scan && presentation.items.length > 0}
        {#if presentation.kind === 'retained'}
          <p class="text-body text-muted-foreground">{presentation.description}</p>
        {/if}
        <div class="category-list rounded-xl border border-border bg-card">
          {#if scanStore.canClean && scanStore.bulkSelection.count > 0}
          <div class="flex flex-wrap items-center gap-x-2 border-b border-border px-1 py-2">
            <Checkbox
              class="min-h-8 gap-3 px-2"
              label="Select all"
              checked={scanStore.bulkSelection.all}
              indeterminate={scanStore.bulkSelection.mixed}
              disabled={!scanStore.canClean || isPreparingReview || scanStore.bulkSelection.count === 0}
              ariaLabel="Select all available cleanup items"
              onchange={(checked) => scanStore.setAllSelected(checked)}
            />
            <span class="text-meta text-muted-foreground">{scanStore.bulkSelection.count} available · running apps excluded</span>
          </div>
          {/if}
          {#each orderedCategories as categoryResult (categoryResult.category)}
            <CategoryCard
              {categoryResult}
              onSelectCategory={(cat) => onSelectCategory(cat)}
            />
          {/each}
          <ProjectBuildOutputCard
            disabled={!scanStore.canClean || isPreparingReview || scanStore.isCleaning}
            onOpenProjects={() => handleTabClick('developer-artifacts')}
          />
        </div>
        {#if projectNotice}
          <p class="mt-2 text-meta text-muted-foreground" role="status">{projectNotice}</p>
        {/if}
      {:else}
        <EmptyState icon={HardDrive} title={presentation.title} description={presentation.description} class="border-solid bg-card py-6" />
      {/if}
    </div>
    {/if}
    {#if scan && !scanStore.isScanning && !scanStore.isCleaning && !scanStore.isRefreshingAfterClean}
      <CleanupGuidanceList {scan} bind:open={itemDetailsOpen} summaryId={`${storagePanelId}-item-details`} onCategory={onSelectCategory} onQuit={platformContextStore.context?.platform === 'macos' ? previewQuit : undefined} onNavigate={onNavigateTab} current={scanStore.canClean} />
    {/if}
    <!-- Review follows the list in both visual and keyboard order. -->
      {#if scan && (presentation.items.some(isActionable) || projectSelection.length > 0) && !scanStore.isScanning && !scanStore.isCleaning && !scanStore.isRefreshingAfterClean}
      <div class="storage-selection">
      <SelectionToolbar
        selectedCount={scanStore.selectedCount + projectSelection.length}
        selectedBytes={scanStore.reclaimableBytes + projectCleanupStore.selectedBytes}
        unestimatedCount={summarizeCategory(scan?.categories.flatMap(category => category.items) ?? [], scanStore.selectedMap).selected_unestimated_count}
        manualCount={scanStore.manualSelectedCount}
        actionLabel="Clean selected"
        onAction={handleCleanSelected}
        isActionDisabled={!scanStore.canClean || (!hasSelectedAction && projectSelection.length === 0) || isPreparingReview || projectCleanupStore.phase === 'cleaning'}
        isActionLoading={scanStore.isCleaning || isPreparingReview || projectCleanupStore.phase === 'cleaning'}
        isSelectionDisabled={!scanStore.canClean}
      >
        {#snippet extraActions()}
          <Button
            variant="ghost"
            size="xs"
            onclick={() => tauriOpenStorageSettings()}
            class="text-muted-foreground"
            title="Open storage settings"
            aria-label="Open storage settings"
          >
            <ExternalLink size={12} />
            <span class="hidden lg:inline">Storage Settings</span>
          </Button>
        {/snippet}
      </SelectionToolbar>
      </div>
      {/if}

  {/if}

  </div>

  {#if review}
    <CleanupReviewDialog
      plan={review.plan}
      items={scan?.categories.flatMap(category => category.items) ?? []}
      disabled={review.scanId !== scan?.scan_id || !scanStore.canClean}
      onCancel={() => (review = null)}
      onConfirm={confirmCleanup}
    />
  {/if}

  {#if showResultModal && scanStore.lastCleanResult}
    <CleanResultModal
      result={scanStore.lastCleanResult}
      onClose={() => (showResultModal = false)}
      returnFocusTargetId="storage-scan-button"
    />
  {/if}
</div>

<style>
  /* Hallmark: modern-minimal workbench; DESIGN.md; designed-as-app. */
  .storage-workspace { container-type: inline-size; }
  @container (min-width: 480px) {
    .storage-workspace :global(.storage-header) { flex-direction: row; align-items: center; }
  }
  .category-list { padding: 0 4px; }
  .storage-selection {
    position: sticky;
    bottom: 0;
    z-index: 2;
    padding: 8px 0;
    background: hsl(var(--background));
  }
</style>

{#if quitPreview}
  <CleanupQuitDialog preview={quitPreview} busy={quitBusy} progress={quitProgress} onCancel={cancelQuit} onConfirm={confirmQuit} />
{/if}
