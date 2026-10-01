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
usageStore.snapshot = await mockApi.getAiUsage();
agentActivityStore.snapshot = await mockApi.getProjectContext();
memoryStore.startPolling = systemMetricsStore.startPolling = () => {};
memoryStore.stopPolling = systemMetricsStore.stopPolling = () => {};
memoryStore.refreshDisk = awakeStore.refresh = async () => {};
usageStore.refreshIfStale = async () => {};
usageStore.observeAutoRefresh = scanStore.observeFreshness = () => () => {};
scanStore.init = async () => {};
scanStore.cancelScan = async () => { scanStore.isCancelling = true; };

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
  if (kind === 'scanning' || kind === 'refreshing' || kind === 'preparing') {
    scanStore.isScanning = true;
    scanStore.scanId = kind === 'preparing' ? null : 'synthetic-validation-scan';
    scanStore.scanStartedAt = Date.now() - 71_000;
    scanStore.isRefreshingAfterClean = kind === 'refreshing';
    scanStore.currentRoot = kind === 'preparing' ? null : { name: 'npm Cache', path: '/fixture/Library/Caches/npm' };
    scanStore.foundItemCount = 53;
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
