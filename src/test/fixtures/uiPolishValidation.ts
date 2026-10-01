// Mounted production components with deterministic browser-only data; never IPC.
import '../../app.css';
import { mount, tick } from 'svelte';
import Dashboard from '../../routes/dashboard/Dashboard.svelte';
import { mockApi } from '../../lib/api/mock';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { platformCapabilitiesStore } from '../../lib/stores/platformCapabilities.svelte';
import { platformContextStore } from '../../lib/stores/platformContext.svelte';
import { memoryStore } from '../../lib/stores/memory.svelte';
import { systemMetricsStore } from '../../lib/stores/systemMetrics.svelte';
import { awakeStore } from '../../lib/stores/awake.svelte';
import { scanStore } from '../../lib/stores/scan.svelte';
import { usageStore } from '../../lib/stores/usage.svelte';
import { agentActivityStore } from '../../lib/stores/agentActivity.svelte';
import type { PublishedScan, ScanEvent, ScanItem, ScanResult } from '../../lib/models/types';

const published = await mockApi.startScan(() => {});
const inventory = published.result;
platformCapabilitiesStore.capabilities = await mockApi.getPlatformCapabilities();
platformContextStore.context = await mockApi.getPlatformContext();
platformCapabilitiesStore.load = platformContextStore.load = async () => {};
settingsStore.hasLoaded = true;
settingsStore.load = async () => {};
settingsStore.settings.theme = 'light';
settingsStore.settings.dashboard_tabs = ['overview', 'storage', 'performance', 'projects', 'docker', 'models', 'development_servers', 'awake'];
settingsStore.settings.ai_accounts_quota_providers = ['codex', 'claude', 'opencode', 'openrouter'];
settingsStore.save = async (preferences) => {
  Object.assign(settingsStore.settings, preferences);
  if (preferences.theme) settingsStore.applyTheme(preferences.theme);
  return true;
};
memoryStore.memory = await mockApi.getMemoryMetrics();
memoryStore.disk = await mockApi.getDiskMetrics();
systemMetricsStore.cpu = await mockApi.getCpuMetrics();
systemMetricsStore.battery = await mockApi.getBatteryMetrics();
awakeStore.state = await mockApi.getAwakeState();
const cachedUsage = await mockApi.getAiUsage();
usageStore.snapshot = structuredClone(cachedUsage);
agentActivityStore.snapshot = await mockApi.getProjectContext();
memoryStore.startPolling = systemMetricsStore.startPolling = () => {};
memoryStore.stopPolling = systemMetricsStore.stopPolling = () => {};
memoryStore.refreshDisk = awakeStore.refresh = async () => {};
usageStore.refreshIfStale = async () => {};
usageStore.observeAutoRefresh = scanStore.observeFreshness = () => () => {};
scanStore.init = async () => {};

// Exercise the real scan workflow with deterministic API events. This measures
// mounted Svelte work, not native IPC or filesystem speed.
let progressRun: Promise<ScanResult | null> | null = null;
let finishProgress: ((value: PublishedScan) => void) | null = null;
let progressObserver: MutationObserver | null = null;
let progressEvidence = {
  scope: 'Mounted production Svelte; synthetic API events; no native IPC',
  events: 0, burstMs: 0, paintedMs: 0, cancelCalls: 0, cancelDispatchedAt: 0,
  stopClickAt: 0, cancelPaintMs: null as number | null,
  statusChanges: [] as string[],
};

async function beginProgress() {
  if (progressRun) throw new Error('Finish the previous synthetic scan first.');
  progressObserver?.disconnect();
  progressEvidence = { scope: progressEvidence.scope, events: 0, burstMs: 0,
    paintedMs: 0, cancelCalls: 0, cancelDispatchedAt: 0, stopClickAt: 0,
    cancelPaintMs: null, statusChanges: [] };
  let emit!: (event: ScanEvent) => void;
  mockApi.startScan = (callback) => {
    emit = callback;
    return new Promise(resolve => { finishProgress = resolve; });
  };
  mockApi.cancelScan = async (scanId) => {
    if (scanId !== 'mounted-progress-fixture') throw new Error('Unexpected scan ID.');
    progressEvidence.cancelCalls++;
    progressEvidence.cancelDispatchedAt = performance.now();
  };
  progressRun = scanStore.runScan();
  emit({ type: 'Started', scan_id: 'mounted-progress-fixture' });
  emit({ type: 'CategoryStarted', category: 'system' });
  await tick();
  const status = document.querySelector('[aria-label="Storage scan progress"] [role="status"]');
  if (!status) throw new Error('Open Storage before starting the progress fixture.');
  progressObserver = new MutationObserver(() => {
    progressEvidence.statusChanges.push(status.textContent?.trim() ?? '');
    if (scanStore.isCancelling && progressEvidence.stopClickAt) {
      progressEvidence.cancelPaintMs = performance.now() - progressEvidence.stopClickAt;
    }
  });
  progressObserver.observe(status, { childList: true, characterData: true, subtree: true });
  document.querySelector('[aria-label="Stop scan"]')?.addEventListener('click', () => {
    progressEvidence.stopClickAt = performance.now();
  }, { capture: true, once: true });
  const item: ScanItem = {
    id: 'fixture', signature_id: 'fixture', name: 'Fixture', category: 'system',
    risk: 'manual', path: '/fixture', size: { logical: 10, allocated: 10 },
    file_count: 1, description: '', is_selected: false, last_modified: null, exists: true,
    quality: 'fresh', incomplete_reason: null,
    disposition: { eligibility: 'advisory', reason: null, cleanable_bytes: null },
  };
  const started = performance.now();
  for (let index = 0; index < 1092; index++) {
    emit({ type: 'RootStarted', category: 'system', signature_id: 'fixture',
      name: 'Measured root', root: `/fixture/${index}` });
    if (index < 466) emit({ type: 'ItemFound', item });
  }
  progressEvidence.events = 1558;
  progressEvidence.burstMs = performance.now() - started;
  await tick();
  await new Promise(requestAnimationFrame);
  progressEvidence.paintedMs = performance.now() - started;
  return progressMeasurement();
}

function progressMeasurement() {
  return { ...progressEvidence, statusChanges: [...progressEvidence.statusChanges],
    foundItems: scanStore.foundItemCount, isScanning: scanStore.isScanning,
    isCancelling: scanStore.isCancelling, canClean: scanStore.canClean,
    selectedCount: scanStore.selectedCount,
    selectedBytes: scanStore.safeSelectedBytes + scanStore.rebuildSelectedBytes + scanStore.manualSelectedBytes,
    cancelled: scanStore.lastScan?.cancelled ?? false,
    progressText: document.querySelector('[aria-label="Storage scan progress"]')?.textContent?.trim(),
  };
}

async function endProgress() {
  if (!progressRun || !finishProgress) throw new Error('No synthetic scan is running.');
  const result = structuredClone(inventory);
  result.scan_id = 'mounted-progress-fixture';
  result.categories = [];
  result.total_bytes = result.safe_bytes = result.rebuild_bytes = result.manual_bytes = 0;
  result.eligibility = undefined;
  result.quality = 'partial';
  result.cancelled = progressEvidence.cancelCalls > 0;
  finishProgress({ result, discovery: { status: 'exhausted' } });
  await progressRun;
  progressRun = null;
  finishProgress = null;
  await tick();
  progressObserver?.disconnect();
  return progressMeasurement();
}

async function scan(kind: string) {
  scanStore.isScanning = false;
  scanStore.isCancelling = false;
  scanStore.isCleaning = false;
  scanStore.isRefreshingAfterClean = false;
  scanStore.error = null;
  scanStore.currentRoot = null;
  scanStore.scanStartedAt = null;
  scanStore.scanId = null;
  const result = structuredClone(inventory);
  result.started_at = Math.floor(Date.now() / 1000) - 2;
  result.finished_at = Math.floor(Date.now() / 1000);
  if (kind === 'empty' || kind === 'unavailable' || kind === 'privacy') {
    result.categories = [];
    result.total_bytes = result.safe_bytes = result.rebuild_bytes = result.manual_bytes = 0;
    result.eligibility = undefined;
  }
  if (kind === 'partial' || kind === 'stopped' || kind === 'unavailable' || kind === 'privacy') {
    result.quality = kind === 'unavailable' || kind === 'privacy' ? 'unavailable' : 'partial';
    result.gaps = [{ kind: kind === 'privacy' ? 'full_disk_access' : 'permission_denied', count: 3 }];
    result.cancelled = kind === 'stopped';
  }
  if (kind === 'retained') {
    for (const category of result.categories) for (const item of category.items) {
      item.is_selected = false;
      item.disposition = { eligibility: 'advisory', reason: 'A dedicated owner operation is required.', cleanable_bytes: 0 };
    }
  }
  // Use the public workflow so failed and subsequently accepted inventories
  // follow the production invalidation rules, including selection authority.
  mockApi.startScan = async () => ({ result, discovery: { status: 'exhausted' } });
  await scanStore.runScan();
  if (kind === 'initial') {
    scanStore.lastScan = null;
    scanStore.selectedMap = {};
  }
  if (kind === 'scanning' || kind === 'refreshing' || kind === 'preparing' || kind === 'stopping') {
    scanStore.isScanning = true;
    scanStore.scanId = kind === 'preparing' ? null : 'synthetic-validation-scan';
    scanStore.scanStartedAt = kind === 'preparing' ? Date.now() : Date.now() - 71_000;
    scanStore.isRefreshingAfterClean = kind === 'refreshing';
    scanStore.currentRoot = kind === 'preparing' ? null : { name: 'npm Cache', path: '/fixture/Library/Caches/npm' };
    scanStore.foundItemCount = kind === 'preparing' ? 0 : 53;
    scanStore.isCancelling = kind === 'stopping';
  }
  if (kind === 'cleaning') {
    scanStore.isCleaning = true;
    scanStore.cleanProgress = { currentItem: 'Verified cache', index: 2, total: 8, percent: 25 };
  }
  if (kind === 'failed') {
    mockApi.startScan = async () => { throw new Error('The storage check did not complete. Try scanning again.'); };
    await scanStore.runScan();
  }
  await tick();
}
await scan('ready');
mount(Dashboard, { target: document.getElementById('app')! });
const driver = {
  ready: false,
  scan,
  beginProgress,
  progressMeasurement,
  endProgress,
  async providerLoading(loading = true) {
    const snapshot = structuredClone(cachedUsage);
    snapshot.fetched_at = Math.floor(Date.now() / 1000);
    usageStore.snapshot = snapshot;
    usageStore.error = null;
    usageStore.isLoading = loading;
    usageStore.loadingProviders = loading ? [...settingsStore.settings.ai_accounts_quota_providers] : [];
    await tick();
  },
  async theme(dark: boolean) { settingsStore.settings.theme = dark ? 'dark' : 'light'; document.documentElement.classList.toggle('dark', dark); await tick(); },
  async values(cpu = 72, memory = 12.3) {
    if (systemMetricsStore.cpu) systemMetricsStore.cpu.usage_percent = cpu;
    if (memoryStore.memory) memoryStore.memory.used_bytes = memory * 1024 ** 3;
    scanStore.foundItemCount += 1;
    await tick();
  },
  async longNames() {
    for (const category of scanStore.lastScan?.categories ?? []) category.display_name = `개발 도구와 애플리케이션 캐시 · ${category.display_name} with an unusually long translated name`;
    for (const provider of usageStore.snapshot?.providers ?? []) provider.name += ' · 개발 조직의 긴 계정 이름과 워크스페이스';
    await tick();
  },
};
(window as unknown as { uiPolishValidation: typeof driver }).uiPolishValidation = driver;
await tick();
driver.ready = true;
