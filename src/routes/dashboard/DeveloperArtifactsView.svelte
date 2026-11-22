<script lang="ts">
  import LoadingState from '../../lib/components/LoadingState.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import TemporaryStorageView from './TemporaryStorageView.svelte';
  import type {
    DeveloperArtifact,
    DeveloperArtifactStatus,
    DeveloperArtifactScanEvent,
    DeveloperArtifactScanResult,
    DeveloperArtifactUninspected,
    DeveloperArtifactUninspectedReason,
    DeveloperWorkspace,
    DeveloperWorkspaceRefusal,
    DeveloperWorkspaceSuggestion,
    RemovalPlanPreview,
    RemovalResult,
  } from '../../lib/models/types';
  import Button from '../../lib/components/Button.svelte';
  import Card from '../../lib/components/Card.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import LoadingIndicator from '../../lib/components/LoadingIndicator.svelte';
  import { formatBytes, formatCountdown, formatTimeAgo, ttlRemaining } from '../../lib/utils/format';
  import {
    refusalForPreview,
    tauriCancelDeveloperArtifactScan,
    tauriExecuteRemovalPlan,
    tauriAddSuggestedDeveloperWorkspace,
    tauriPickDeveloperWorkspace,
    tauriPrepareDeveloperArtifactCleanup,
    tauriRegisterDeveloperHomeWorkspace,
    tauriRestoreDeveloperWorkspaces,
    tauriSuggestDeveloperWorkspaces,
    tauriShowInFileManager,
    tauriStartDeveloperArtifactScan,
  } from '../../lib/utils/tauri';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import { developerArtifactKindLabel } from '../../lib/utils/developerArtifactKinds';
  import { settingsStore } from '../../lib/stores/settings.svelte';
  import { observeWhileVisible } from '../../lib/utils/visiblePolling';
  import { nonOverlappingArtifactIds, remainingArtifactScopes, toggleArtifactSelection } from '../../lib/utils/developerArtifactSelection';
  import { canReveal, revealUnavailableReason, runReveal } from '../../lib/utils/reveal';
  import {
    AlertCircle,
    CheckSquare,
    FolderOpen,
    HardDrive,
    RefreshCw,
    ShieldCheck,
    Square,
    Trash2,
    X,
  } from '@lucide/svelte';

  interface Props {
    onBack: () => void;
    /**
     * The last scan result, when the route reopens this view with one. It seeds
     * the review list and the uninspected notice so a partial scan is explained
     * before the next scan finishes.
     */
    initialResult?: DeveloperArtifactScanResult | null;
  }

  let { onBack, initialResult = null }: Props = $props();
  let temporaryReviewOpen = $state(false);

  onMount(() => {
    void platformContextStore.load();
    void restoreSavedFolders();
  });

  type SortKey = 'size' | 'activity' | 'project' | 'type';

  let workspaces = $state<DeveloperWorkspace[]>([]);
  // Likely project folders that are not saved yet (#486). Hints only.
  let suggestions = $state<DeveloperWorkspaceSuggestion[]>([]);
  // Saved folders that no longer validate; they stay saved until removed.
  let restoreRefusals = $state<DeveloperWorkspaceRefusal[]>([]);
  // Saved folders are being registered again; the empty hint would mislead.
  let isRestoring = $state(true);
  let selectedWorkspaceIds = $state<string[]>([]);
  // The last result seeds this view once; every scan after that owns the state,
  // so the prop read is intentionally untracked.
  const seed = untrack(() => initialResult);
  let items = $state<DeveloperArtifact[]>(seed?.items ?? []);
  let scanResult = $state<DeveloperArtifactScanResult | null>(seed);
  let uninspected = $state<DeveloperArtifactUninspected[]>(seed?.uninspected ?? []);
  let plan = $state<RemovalPlanPreview | null>(null);
  let removalResult = $state<RemovalResult | null>(null);
  let relatedScopesNeedRefresh = $state(false);
  let selectedIds = $state<string[]>([]);
  let sortBy = $state<SortKey>('size');
  let isScanning = $state(false);
  let isPreparing = $state(false);
  let isExecuting = $state(false);
  let activeScanId = $state<string | null>(null);
  let activeWorkspace = $state('');
  // What the scan is doing right now, so a long measurement never looks idle.
  let scanActivity = $state('');
  let discoveredCount = $state(seed?.discovered_count ?? 0);
  let measuredCount = $state(seed?.measured_count ?? 0);
  let skippedEntries = $state(seed?.skipped_entries ?? 0);
  let error = $state<string | null>(null);
  let revealError = $state<string | null>(null);
  let now = $state(Date.now());
  let expiryActionFocused = $state(false);
  let partialCleanupConfirmed = $state(false);
  let selectedIdSet = $derived(new Set(selectedIds));
  let selectedItems = $derived(items.filter((item) => selectedIdSet.has(item.id)));
  let selectedBytes = $derived(selectedItems.reduce((total, item) => total + item.allocated_bytes, 0));
  let hasMeasurementIncompleteSelected = $derived(
    selectedItems.some((item) => item.status === 'measurement_incomplete')
  );
  let hasFrameworkSelection = $derived(selectedItems.some(item => ['svelte_kit_output', 'svelte_kit_types', 'next_output', 'next_webpack_cache'].includes(item.kind)));
  let sortedItems = $derived(
    [...items].sort((left, right) => {
      if (sortBy === 'activity') {
        return (right.newest_mtime ?? 0) - (left.newest_mtime ?? 0);
      }
      if (sortBy === 'project') {
        return left.project_name.localeCompare(right.project_name);
      }
      if (sortBy === 'type') {
        return kindLabel(left).localeCompare(kindLabel(right));
      }
      return right.allocated_bytes - left.allocated_bytes;
    })
  );
  let remainingSecs = $derived(plan ? ttlRemaining(plan.expires_at, now) : 0);
  let isExpired = $derived(plan ? remainingSecs === 0 : false);

  $effect(() => {
    if (!plan) return;
    return observeWhileVisible(() => { now = Date.now(); }, 1000);
  });

  $effect(() => {
    if (!isExpired || expiryActionFocused) return;
    expiryActionFocused = true;
    void tick().then(() => document.getElementById('developer-artifact-expiry-action')?.focus());
  });

  function kindLabel(item: DeveloperArtifact): string {
    return developerArtifactKindLabel(item.kind);
  }

  function cleanupScopeLabel(item: DeveloperArtifact): string {
    if (item.status === 'observation_only' && (item.kind === 'svelte_kit_output' || item.kind === 'next_output')) return 'Observed output · no cleanup scope';
    const labels: Record<DeveloperArtifact['kind'], string> = {
      cargo_target: 'target/',
      node_modules: 'node_modules/',
      svelte_kit_output: '.svelte-kit/',
      svelte_kit_types: '.svelte-kit/types/',
      next_output: '.next/',
      next_webpack_cache: '.next/cache/webpack/',
      python_venv: item.path.endsWith('/.venv') ? '.venv/' : 'venv/',
      go_module_cache: 'pkg/mod/',
      maven_target: 'target/',
      sbt_target: 'target/',
      clojure_target: 'target/',
      gradle_build: 'build/',
      gradle_cache: '.gradle/',
      composer_vendor: 'vendor/',
      ruby_bundle: 'vendor/bundle/',
      dotnet_bin: 'bin/',
      dotnet_obj: 'obj/',
      c_make_build: 'build/',
      swift_build: '.build/',
      flutter_tooling: '.dart_tool/',
      elixir_build: '_build/',
      elixir_deps: 'deps/',
      erlang_build: '_build/',
      haskell_stack_work: '.stack-work/',
      haskell_dist_newstyle: 'dist-newstyle/',
      zig_cache: '.zig-cache/',
      terraform_cache: '.terraform/',
    };
    return labels[item.kind];
  }

  function ecosystemLabel(item: DeveloperArtifact): string {
    return item.ecosystem === 'dotnet'
      ? '.NET'
      : item.ecosystem.charAt(0).toUpperCase() + item.ecosystem.slice(1);
  }

  function resetReview() {
    plan = null;
    removalResult = null;
    relatedScopesNeedRefresh = false;
    expiryActionFocused = false;
    partialCleanupConfirmed = false;
  }

  function canManuallyClean(item: DeveloperArtifact): boolean {
    return item.ownership.state === 'verified_generated' && (item.status === 'complete' || item.status === 'measurement_incomplete');
  }

  function statusLabel(status: DeveloperArtifactStatus): string {
    switch (status) {
      case 'measurement_incomplete':
        return 'Partial measurement';
      case 'safety_blocked':
        return 'Blocked · safety check';
      case 'scan_cancelled':
        return 'Blocked · scan incomplete';
      case 'observation_only':
        return 'Observed only · cleanup unavailable';
      default:
        return '';
    }
  }

  function uninspectedReasonLabel(reason: DeveloperArtifactUninspectedReason): string {
    return reason === 'permission_denied'
      ? 'The operating system refused access, so this folder was not inspected.'
      : 'This folder could not be read, so it was not inspected.';
  }

  function uninspectedStatusLabel(entry: DeveloperArtifactUninspected): string {
    if (entry.retryable) return 'Permission refused · retry allowed';
    return 'Unreadable';
  }

  function savedProjectFolders(): string[] {
    return settingsStore.settings.project_folders ?? [];
  }

  /** Remembers a folder so the next launch scans it again (#486). */
  async function rememberFolder(workspace: DeveloperWorkspace) {
    const saved = savedProjectFolders();
    if (saved.includes(workspace.path)) return;
    await settingsStore.save({ project_folders: [...saved, workspace.path] });
  }

  async function forgetFolder(path: string) {
    const saved = savedProjectFolders();
    if (!saved.includes(path)) return;
    await settingsStore.save({ project_folders: saved.filter((folder) => folder !== path) });
  }

  function includeWorkspaces(added: DeveloperWorkspace[]) {
    const known = new Set(workspaces.map((workspace) => workspace.id));
    workspaces = [...workspaces, ...added.filter((workspace) => !known.has(workspace.id))];
    selectedWorkspaceIds = [...new Set([...selectedWorkspaceIds, ...added.map((workspace) => workspace.id)])];
  }

  async function refreshSuggestions() {
    try {
      suggestions = await tauriSuggestDeveloperWorkspaces();
    } catch {
      suggestions = [];
    }
  }

  /**
   * Registers the saved project folders again and scans them, so a folder
   * added once is found on every visit without another choice (#486).
   */
  async function restoreSavedFolders() {
    let restored = false;
    try {
      const restore = await tauriRestoreDeveloperWorkspaces();
      restoreRefusals = restore.refused;
      if (restore.workspaces.length > 0) {
        includeWorkspaces(restore.workspaces);
        restored = true;
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      isRestoring = false;
    }
    await refreshSuggestions();
    if (restored && !scanResult && !isScanning) await runScan(selectedWorkspaceIds);
  }

  async function addWorkspace() {
    error = null;
    try {
      const workspace = await tauriPickDeveloperWorkspace();
      if (!workspace) return;
      includeWorkspaces([workspace]);
      await rememberFolder(workspace);
      await refreshSuggestions();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function addSuggestedFolder(path: string) {
    error = null;
    try {
      const workspace = await tauriAddSuggestedDeveloperWorkspace(path);
      includeWorkspaces([workspace]);
      await rememberFolder(workspace);
      await refreshSuggestions();
      await runScan(selectedWorkspaceIds);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function forgetRefusedFolder(path: string) {
    await forgetFolder(path);
    restoreRefusals = restoreRefusals.filter((refusal) => refusal.path !== path);
    await refreshSuggestions();
  }

  function removeWorkspace(id: string) {
    const removed = workspaces.find((workspace) => workspace.id === id);
    if (removed) void forgetFolder(removed.path).then(refreshSuggestions);
    workspaces = workspaces.filter((workspace) => workspace.id !== id);
    selectedWorkspaceIds = selectedWorkspaceIds.filter((workspaceId) => workspaceId !== id);
    if (selectedWorkspaceIds.length === 0) {
      items = [];
      scanResult = null;
      selectedIds = [];
    }
  }

  function handleScanEvent(event: DeveloperArtifactScanEvent) {
    switch (event.type) {
      case 'started':
        activeScanId = event.scan_id;
        discoveredCount = 0;
        measuredCount = 0;
        skippedEntries = 0;
        scanActivity = '';
        break;
      case 'workspace_started':
        activeWorkspace = event.workspace.display_path;
        break;
      case 'artifact_found':
        if (!items.some((item) => item.id === event.artifact.id)) {
          items = [...items, event.artifact];
        }
        break;
      case 'progress':
        activeWorkspace = workspaces.find((workspace) => workspace.id === event.workspace_id)?.display_path ?? activeWorkspace;
        discoveredCount = event.discovered_count;
        measuredCount = event.measured_count;
        skippedEntries = event.skipped_entries;
        break;
      case 'uninspected':
        if (!uninspected.some((entry) => entry.path === event.path)) {
          uninspected = [
            ...uninspected,
            {
              path: event.path,
              name: event.name,
              reason: event.reason,
              retryable: event.retryable,
            },
          ];
        }
        break;
      case 'finished':
        scanResult = event.result;
        items = event.result.items;
        // Idle, verified build output is preselected by the backend (#494);
        // dependency stores and anything uncertain stay unselected.
        selectedIds = nonOverlappingArtifactIds(
          event.result.items.filter((item) => item.selected_by_default && canManuallyClean(item))
        );
        uninspected = event.result.uninspected ?? [];
        discoveredCount = event.result.discovered_count;
        measuredCount = event.result.measured_count;
        skippedEntries = event.result.skipped_entries;
        break;
      case 'cancelled':
        activeScanId = event.scan_id;
        break;
      case 'project_discovered':
        scanActivity = `Found ${event.project_name}`;
        break;
      case 'artifact_measurement_started':
        scanActivity = `Measuring ${event.project_name} · ${developerArtifactKindLabel(event.kind)}`;
        break;
      case 'workspace_finished':
        break;
    }
  }

  async function runScan(workspaceIds: string[]) {
    if (workspaceIds.length === 0) {
      error = 'Add at least one workspace before scanning.';
      return;
    }
    isScanning = true;
    error = null;
    resetReview();
    scanResult = null;
    items = [];
    uninspected = [];
    selectedIds = [];
    activeScanId = null;
    activeWorkspace = '';
    discoveredCount = 0;
    measuredCount = 0;
    skippedEntries = 0;
    try {
      const result = await tauriStartDeveloperArtifactScan(workspaceIds, handleScanEvent);
      scanResult = result;
      items = result.items;
      // The command result is authoritative. Channel delivery is best-effort,
      // so do not rely on the `uninspected` progress event having arrived.
      uninspected = result.uninspected ?? [];
      discoveredCount = result.discovered_count;
      measuredCount = result.measured_count;
      skippedEntries = result.skipped_entries;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      isScanning = false;
      activeScanId = null;
      activeWorkspace = '';
      scanActivity = '';
    }
  }

  async function scanArtifacts() {
    await runScan(selectedWorkspaceIds);
  }

  async function scanThisComputer() {
    error = null;
    try {
      const workspace = await tauriRegisterDeveloperHomeWorkspace();
      workspaces = [workspace];
      selectedWorkspaceIds = [workspace.id];
      await runScan([workspace.id]);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  /** Re-runs the scan that reported the uninspected folders. */
  async function rescanAfterUninspected() {
    await runScan(selectedWorkspaceIds);
  }

  async function cancelScan() {
    if (!activeScanId) return;
    try {
      await tauriCancelDeveloperArtifactScan(activeScanId);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function toggleItem(item: DeveloperArtifact) {
    if (!canManuallyClean(item) || isScanning || isExecuting) return;
    selectedIds = toggleArtifactSelection(selectedIds, item, items);
    resetReview();
  }

  function selectAll() {
    const eligible = items.filter((item) => item.status === 'complete' && canManuallyClean(item));
    selectedIds = nonOverlappingArtifactIds(eligible);
    resetReview();
  }

  function deselectAll() {
    selectedIds = [];
    resetReview();
  }

  async function reviewCleanup() {
    if (!scanResult || selectedIds.length === 0) return;
    isPreparing = true;
    error = null;
    try {
      plan = await tauriPrepareDeveloperArtifactCleanup(scanResult.scan_id, selectedIds);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      isPreparing = false;
    }
  }

  async function executeCleanup() {
    if (!plan) return;
    const refusal = refusalForPreview('Deleting artifacts');
    if (refusal) {
      error = refusal;
      return;
    }
    if (hasMeasurementIncompleteSelected && !partialCleanupConfirmed) {
      error = `Confirm the partial-measurement warning before permanently deleting these artifacts.`;
      return;
    }
    isExecuting = true;
    error = null;
    try {
      const result = await tauriExecuteRemovalPlan(plan.id);
      removalResult = result;
      const moved = new Set(result.items.filter((item) => item.success).map((item) => item.item_id));
      const remaining = remainingArtifactScopes(items, moved);
      relatedScopesNeedRefresh = items.length - remaining.length > moved.size;
      items = remaining;
      const remainingIds = new Set(items.map(item => item.id));
      selectedIds = selectedIds.filter((id) => remainingIds.has(id));
      plan = null;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      isExecuting = false;
    }
  }
</script>

{#if temporaryReviewOpen}
  <TemporaryStorageView onBack={() => temporaryReviewOpen = false} />
{:else}
<div class="space-y-5">
  <div class="flex items-start gap-3">
    <div class="min-w-0 flex-1">
      <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <h1 class="text-title font-semibold tracking-tight">Projects</h1>
        <div class="flex shrink-0 items-center gap-1.5 text-meta text-muted-foreground">
          <ShieldCheck size={14} class="text-success" />
          <span>Review only · nothing selected by default</span>
        </div>
      </div>
      <p class="mt-1 text-meta text-muted-foreground">
        Inspect rebuildable project environments across common ecosystems. Project source, manifests, lockfiles, and project roots are never cleanup targets.
        Verified framework sync/cache-only defaults can be permanently deleted as whole units; deployment, offline, dynamic/custom and unknown output stays observed. Choose one whole folder or generated child per directory.
      </p>
    </div>
  </div>

  <Button variant="outline" disabled={isScanning || isPreparing || isExecuting} onclick={() => temporaryReviewOpen = true}>Review temporary folders</Button>

  <Card class="p-5 space-y-4">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <div class="text-meta font-medium">Scan scope</div>
        <p class="mt-1 text-meta text-muted-foreground">Scan your user-owned files in one pass. System, credential, media, and installed-application paths are bypassed.</p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        {#if isScanning && activeScanId}
          <Button variant="outline" size="md" onclick={cancelScan} class="gap-1.5">
            <X size={14} />
            Cancel
          </Button>
        {/if}
        <Button variant="primary" size="md" onclick={scanThisComputer} disabled={isScanning} class="gap-1.5">
          <LoadingActionContent busy={isScanning} busyLabel="Scanning…" word="scanning">
            <HardDrive size={14} aria-hidden="true" />Scan this computer
          </LoadingActionContent>
        </Button>
        <Button variant="outline" size="md" onclick={addWorkspace} disabled={isScanning} class="gap-1.5">
          <FolderOpen size={14} />
          Add folder
        </Button>
        <Button variant="outline" size="md" onclick={scanArtifacts} disabled={isScanning || selectedWorkspaceIds.length === 0} class="gap-1.5">
          <LoadingActionContent busy={isScanning} busyLabel="Scanning…" word="scanning">
            <RefreshCw size={14} aria-hidden="true" />Scan selected
          </LoadingActionContent>
        </Button>
      </div>
    </div>

    {#if suggestions.length > 0}
      <div class="flex flex-wrap items-center gap-2" aria-label="Suggested project folders">
        <span class="text-meta text-muted-foreground">Suggested</span>
        {#each suggestions as suggestion (suggestion.path)}
          <Button
            variant="outline"
            size="sm"
            onclick={() => addSuggestedFolder(suggestion.path)}
            disabled={isScanning}
            class="max-w-full gap-1.5"
            title={`Add ${suggestion.display_path} and scan it`}
          >
            <FolderOpen size={13} aria-hidden="true" />
            <span class="truncate font-mono">{suggestion.display_path}</span>
            <span class="shrink-0 text-muted-foreground">· {suggestion.project_count} {suggestion.project_count === 1 ? 'project' : 'projects'}</span>
          </Button>
        {/each}
      </div>
    {/if}

    {#if restoreRefusals.length > 0}
      <div class="status-panel-warning space-y-1 rounded-lg border p-3 text-meta" role="status">
        {#each restoreRefusals as refusal (refusal.path)}
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="min-w-0 break-all"><span class="font-mono">{refusal.path}</span> · {refusal.reason}</span>
            <Button variant="ghost" size="sm" onclick={() => forgetRefusedFolder(refusal.path)}>Forget</Button>
          </div>
        {/each}
      </div>
    {/if}

    {#if isRestoring && workspaces.length === 0}
      <div class="flex items-center gap-2 rounded-lg border border-dashed border-border/80 p-4 text-meta text-muted-foreground" role="status">
        <LoadingIndicator size="xs" class="text-primary" />
        Restoring saved project folders…
      </div>
    {:else if workspaces.length === 0}
      <div class="rounded-lg border border-dashed border-border/80 p-4 text-meta text-muted-foreground">
        <span class="font-medium text-foreground">Add folder</span> once and neati remembers it, scanning it whenever you open Projects. <span class="font-medium text-foreground">Scan this computer</span> searches all of your user-owned files in one pass.
      </div>
    {:else}
      <div class="grid gap-2 sm:grid-cols-2">
        {#each workspaces as workspace (workspace.id)}
          <div class="flex items-center gap-2 rounded-lg border border-border/70 bg-secondary/20 px-3 py-2">
            <input
              type="checkbox"
              checked={selectedWorkspaceIds.includes(workspace.id)}
              onchange={() => {
                selectedWorkspaceIds = selectedWorkspaceIds.includes(workspace.id)
                  ? selectedWorkspaceIds.filter((id) => id !== workspace.id)
                  : [...selectedWorkspaceIds, workspace.id];
              }}
              disabled={isScanning}
              aria-label={`Scan ${workspace.name}`}
              class="accent-success"
            />
            <div class="min-w-0 flex-1">
              <div class="truncate text-meta font-medium">{workspace.name}</div>
              <div class="truncate font-mono text-caption text-muted-foreground">{workspace.display_path}</div>
            </div>
            <Button variant="ghost" size="icon" class="h-7 w-7" onclick={() => removeWorkspace(workspace.id)} disabled={isScanning} aria-label={`Remove ${workspace.name}`} title="Remove workspace">
              <X size={13} />
            </Button>
          </div>
        {/each}
      </div>
    {/if}

    {#if isScanning}
      <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border/60 pt-3 text-meta text-muted-foreground">
        <span class="flex min-w-0 items-center gap-2" role="status" aria-live="polite">
          <LoadingIndicator size="xs" word="scanning" class="shrink-0 text-primary" />
          <span class="truncate">{scanActivity || `Scanning ${activeWorkspace || 'selected workspaces'}…`}</span>
        </span>
        <span class="font-mono">{measuredCount} / {discoveredCount} measured · {skippedEntries} skipped</span>
      </div>
    {/if}
  </Card>

  {#if error}
    <div class="flex items-center gap-2.5 rounded-xl border status-chip-destructive p-3.5 text-meta text-destructive">
      <AlertCircle size={16} class="shrink-0" />
      <span>{error}</span>
    </div>
  {/if}

  {#if revealError}
    <div role="alert" class="flex items-center gap-2.5 rounded-xl border status-chip-destructive p-3.5 text-meta text-destructive">
      <AlertCircle size={16} class="shrink-0" />
      <span>{revealError}</span>
    </div>
  {/if}

  {#if uninspected.length > 0}
    <div role="status" data-testid="developer-artifact-uninspected-notice">
      <Card class="space-y-3 status-panel-warning p-4">
        <div class="flex items-start gap-2.5">
          <AlertCircle size={16} class="mt-0.5 shrink-0 text-warning" />
          <div class="min-w-0 flex-1 space-y-2">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-meta font-medium text-warning">
                {uninspected.length} folder{uninspected.length === 1 ? '' : 's'} could not be inspected
              </span>
              <span class="text-meta text-warning/90">Result is partial</span>
            </div>
            <p class="text-meta text-muted-foreground">
              Projects inside these folders were not measured, so their build output and dependencies are missing from this result.
            </p>
            <div class="space-y-1.5">
              {#each uninspected as entry (entry.path)}
                <div class="rounded-lg border border-border/70 bg-background/60 p-2.5 text-caption">
                  <div class="flex flex-wrap items-center justify-between gap-2">
                    <span class="font-medium text-foreground">{entry.name}</span>
                    <span class={`rounded border px-1.5 py-0.5 ${entry.retryable ? 'status-chip-warning text-warning' : 'border-border bg-secondary text-muted-foreground'}`}>
                      {uninspectedStatusLabel(entry)}
                    </span>
                  </div>
                  <div class="mt-1 truncate font-mono text-muted-foreground" title={entry.path}>{entry.path}</div>
                  <p class="mt-1 text-muted-foreground">
                    {uninspectedReasonLabel(entry.reason)}
                    {entry.retryable ? ' Allow access, then scan again.' : ''}
                  </p>
                </div>
              {/each}
            </div>
            {#if uninspected.some((entry) => entry.retryable && entry.reason === 'permission_denied')}
              <p class="text-meta text-muted-foreground">
                On macOS, allow neati under System Settings → Privacy &amp; Security → Files and Folders, then scan again.
              </p>
            {/if}
            <Button
              variant="outline"
              size="sm"
              class="gap-1.5"
              onclick={rescanAfterUninspected}
              disabled={isScanning || selectedWorkspaceIds.length === 0}
            >
              <LoadingActionContent busy={isScanning} busyLabel="Scanning…" word="scanning">
                <RefreshCw size={13} aria-hidden="true" />Scan again
              </LoadingActionContent>
            </Button>
          </div>
        </div>
      </Card>
    </div>
  {/if}

  {#if removalResult}
    <Card class={`p-4 ${removalResult.failed_count + removalResult.skipped_count > 0 ? 'status-panel-warning' : 'status-panel-success'}`}>
      <div class="flex items-center justify-between gap-3 text-meta">
        <span class={`font-medium ${removalResult.failed_count + removalResult.skipped_count > 0 ? 'text-warning' : 'text-success'}`}>
          Permanently removed {removalResult.removed_count} artifact{removalResult.removed_count === 1 ? '' : 's'}
        </span>
        <span class="font-mono text-muted-foreground">{removalResult.size_is_lower_bound ? '≥ ' : ''}{formatBytes(removalResult.removed_allocated_size)} removed · free-space change not measured</span>
      </div>
      {#if relatedScopesNeedRefresh}<p class="mt-2 text-meta text-muted-foreground">Scan again to refresh related parent and child output scopes.</p>{/if}
      {#if removalResult.items.some(item => !item.success)}
        <details class="mt-2 text-meta text-muted-foreground">
          <summary class="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">Kept artifacts and recovery details</summary>
          <ul class="mt-1 space-y-1">{#each removalResult.items.filter(item => !item.success) as item (item.item_id)}<li class="break-words">{item.message}</li>{/each}</ul>
        </details>
      {/if}
    </Card>
  {/if}

  {#if plan}
    <Card class={`space-y-3 p-5 ${isExpired ? 'status-panel-destructive' : 'status-panel-warning'}`}>
      <div class="flex flex-col justify-between gap-3 lg:flex-row lg:items-center">
        <div>
          <div class="flex items-center gap-2 text-sm font-semibold">
            Generated-folder review ready
            <span class="rounded border border-border bg-secondary px-1.5 py-0.5 font-mono text-caption text-muted-foreground">{formatCountdown(remainingSecs)}</span>
          </div>
          <p class="mt-1 text-meta text-muted-foreground">{plan.item_count} selected artifact{plan.item_count === 1 ? '' : 's'} · {plan.size_is_lower_bound ? '≥ ' : ''}{formatBytes(plan.allocated_size)} allocated</p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="ghost" size="sm" disabled={isExecuting} onclick={() => { plan = null; partialCleanupConfirmed = false; }}>Cancel</Button>
          <Button variant="destructive" size="md" onclick={executeCleanup} disabled={isExecuting || isExpired || (hasMeasurementIncompleteSelected && !partialCleanupConfirmed)} class="gap-1.5">
            <LoadingActionContent busy={isExecuting} busyLabel="Deleting…" word="cleaning">
              <Trash2 size={14} aria-hidden="true" />{isExpired ? 'Expired' : 'Permanently delete generated folders'}
            </LoadingActionContent>
          </Button>
        </div>
      </div>
      <div class="rounded-lg border border-border/70 bg-background/60 p-3">
        <p class="text-meta text-muted-foreground">Only the exact generated directories below will be permanently deleted. Project source and project configuration stay; verified generated metadata is deleted with its output.</p>
        {#if hasFrameworkSelection}<p class="mt-1 text-meta text-muted-foreground">Deletion cannot be undone from Trash. Rebuild verified generated outputs with their owning tools.</p>{/if}
        <div class="mt-2 max-h-32 space-y-1.5 overflow-y-auto scroll-stable">
          {#each selectedItems as item (item.id)}
            <div class="flex items-center justify-between gap-3 text-caption">
              <span class="min-w-0 truncate font-mono" title={item.path}>{item.path}</span>
              <span class="shrink-0 text-muted-foreground">{formatBytes(item.allocated_bytes)}</span>
            </div>
          {/each}
        </div>
      </div>
      {#if isExpired}
        <div role="alert" class="flex items-center justify-between gap-2 rounded-lg border status-chip-destructive p-2.5 text-meta text-destructive">
          <span>Plan expired. Scan again to refresh the inventory.</span>
          <Button id="developer-artifact-expiry-action" variant="ghost" size="sm" onclick={() => { plan = null; void scanArtifacts(); }}>Scan again</Button>
        </div>
      {:else}
        <p class="text-meta text-muted-foreground">One-shot plan. neati rechecks workspace identity, markers, exact artifact type, symlinks, and filesystem identity before each permanent deletion.</p>
        {#if hasMeasurementIncompleteSelected}
          <div class="space-y-2 rounded-lg border status-chip-warning p-3 text-meta text-warning">
            <p class="font-medium">Some selected measurements are partial.</p>
            <p class="text-warning/90">The generated-folder scope and project evidence were verified, but one or more entries could not be measured. The displayed size and file count may be lower than the actual contents. Nested links are not followed.</p>
            <label class="flex items-start gap-2 text-warning">
              <input type="checkbox" bind:checked={partialCleanupConfirmed} class="mt-0.5 accent-warning" />
              <span>I understand the measurements may be partial and want to permanently delete the verified generated folder(s).</span>
            </label>
          </div>
        {/if}
      {/if}
    </Card>
  {/if}

  <div class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h2 class="text-sm font-semibold">Reviewable project artifacts</h2>
      <p class="mt-0.5 text-meta text-muted-foreground">
        {#if scanResult}
          {items.length} candidate{items.length === 1 ? '' : 's'} · {measuredCount} measured{uninspected.length > 0 ? ` · ${uninspected.length} uninspected` : ''}{scanResult.cancelled ? ' · scan cancelled' : ''}{scanResult.truncated ? ' · result cap reached' : ''}
        {:else}
          Rust, Node, Python, Java/Kotlin, PHP, Ruby, .NET, C/C++, Swift, Dart, Elixir, Terraform, and Go markers are supported.
        {/if}
      </p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <label for="developer-artifact-sort" class="text-meta text-muted-foreground">Sort</label>
      <select id="developer-artifact-sort" bind:value={sortBy} class="h-8 rounded-lg border border-border bg-background px-2.5 text-meta">
        <option value="size">Largest first</option>
        <option value="activity">Most recently changed</option>
        <option value="project">Project name</option>
        <option value="type">Artifact type</option>
      </select>
      {#if items.length > 0}
        <Button variant="ghost" size="sm" onclick={selectAll} disabled={isScanning || isExecuting}><CheckSquare size={13} /> Select rebuildable</Button>
        <Button variant="ghost" size="sm" onclick={deselectAll} disabled={isScanning || isExecuting}><Square size={13} /> Clear</Button>
        <Button variant="primary" size="md" onclick={reviewCleanup} disabled={isScanning || isPreparing || isExecuting || selectedIds.length === 0} class="gap-1.5">
          <LoadingActionContent busy={isPreparing} busyLabel="Reviewing…" word="working">
            <Trash2 size={14} aria-hidden="true" />Review {formatBytes(selectedBytes)}
          </LoadingActionContent>
        </Button>
      {/if}
    </div>
  </div>

  {#if sortedItems.length === 0}
    {#if isScanning}
      <LoadingState message={scanActivity || 'Scanning project folders…'} word="scanning" detail="Build output and dependencies appear here as each project is measured." />
    {:else}
      <Card class="p-8 text-center text-meta text-muted-foreground">No recognized developer artifacts yet. Add a workspace and run a scan.</Card>
    {/if}
  {:else}
    <div class="developer-artifact-list overflow-hidden rounded-xl border border-border bg-card divide-y divide-border">
      {#each sortedItems as item (item.id)}
        <article
          class={`developer-artifact-row flex items-start gap-3 border-l-2 px-4 py-3 transition-colors ${selectedIdSet.has(item.id) ? 'bg-accent/75' : 'hover:bg-secondary/55'} ${selectedIdSet.has(item.id) ? 'border-l-primary' : 'border-l-transparent'}`}
        >
          <input
            type="checkbox"
            checked={selectedIdSet.has(item.id)}
            onchange={() => toggleItem(item)}
            disabled={!canManuallyClean(item) || isScanning || isExecuting}
            aria-label={`Select ${item.project_name} ${kindLabel(item)}${item.status === 'measurement_incomplete' ? ' with partial measurement warning' : ''}`}
            class="mt-1 accent-primary"
          />
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-meta font-semibold">{item.project_name}</span>
              <span class="text-caption text-muted-foreground">{ecosystemLabel(item)} · {kindLabel(item)}</span>
              {#if item.status !== 'complete'}
                <span class={`rounded border px-1.5 py-0.5 text-caption ${item.status === 'measurement_incomplete' || item.status === 'observation_only' ? 'border-border bg-secondary/60 text-muted-foreground' : 'status-chip-destructive text-destructive'}`}>{statusLabel(item.status)}</span>
              {/if}
            </div>
            <div class="truncate font-mono text-caption text-muted-foreground" title={item.path}>{item.path}</div>
            <div class="flex flex-wrap gap-x-3 gap-y-1 text-meta text-muted-foreground">
              <span>{item.file_count.toLocaleString()} files</span>
              <span>Last changed {formatTimeAgo(item.newest_mtime ?? undefined)}</span>
              <span>Observed unit: <span class="font-mono text-foreground">{cleanupScopeLabel(item)}</span></span>
            </div>
            {#if item.incomplete_reason}
              <p class={`text-caption ${item.status === 'measurement_incomplete' || item.status === 'observation_only' ? 'text-muted-foreground' : 'text-destructive'}`}>
                {item.incomplete_reason}
              </p>
            {/if}
            <details class="text-caption text-muted-foreground">
              <summary class="w-fit cursor-pointer rounded-sm py-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                Evidence and rebuild details
              </summary>
              <div class="mt-1 space-y-1 pl-2">
                <p>Evidence: {item.evidence.join(' · ')}</p>
                {#if item.rebuild_hint}<p>Rebuild: <span class="font-mono text-foreground">{item.rebuild_hint}</span></p>{/if}
              </div>
            </details>
          </div>
          <div class="min-w-16 shrink-0 pt-0.5 text-right">
            <span class="block text-body font-semibold tabular-nums text-foreground">{formatBytes(item.allocated_bytes)}</span>
            <span class="text-caption text-muted-foreground">allocated</span>
          </div>
          <Button
            variant="ghost"
            size="icon"
            class="h-7 w-7 shrink-0"
            disabled={!canReveal()}
            onclick={() => void runReveal(() => tauriShowInFileManager(item.path), (message) => (revealError = message))}
            aria-label={`Show ${item.path} in file manager`}
            title={canReveal() ? platformContextStore.revealLabel : revealUnavailableReason()}
          >
            <FolderOpen size={13} />
          </Button>
        </article>
      {/each}
    </div>
  {/if}
</div>
{/if}
