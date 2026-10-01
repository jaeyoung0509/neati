import '../../app.css';
import { mount, tick } from 'svelte';
import QuickPanel from '../../routes/quick/QuickPanel.svelte';
import { mockApi } from '../../lib/api/mock';
import { nativeApi } from '../../lib/api/native';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { platformCapabilitiesStore } from '../../lib/stores/platformCapabilities.svelte';
import { platformContextStore } from '../../lib/stores/platformContext.svelte';
import { memoryStore } from '../../lib/stores/memory.svelte';
import { systemMetricsStore } from '../../lib/stores/systemMetrics.svelte';
import { awakeStore } from '../../lib/stores/awake.svelte';
import { scanStore } from '../../lib/stores/scan.svelte';
import { usageStore } from '../../lib/stores/usage.svelte';
import { windowProbe } from './quickWindowMock';

Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
settingsStore.settings.quick_panel_ai_providers = ['codex', 'antigravity'];
settingsStore.settings.ai_accounts_quota_providers = ['codex', 'antigravity'];
settingsStore.hasLoaded = true;
settingsStore.load = async () => {};
platformCapabilitiesStore.capabilities = await mockApi.getPlatformCapabilities();
platformCapabilitiesStore.load = async () => {};
platformContextStore.context = await mockApi.getPlatformContext();
platformContextStore.load = async () => {};
memoryStore.memory = await mockApi.getMemoryMetrics();
memoryStore.disk = await mockApi.getDiskMetrics();
systemMetricsStore.cpu = await mockApi.getCpuMetrics();
systemMetricsStore.battery = await mockApi.getBatteryMetrics();
awakeStore.state = await mockApi.getAwakeState();
memoryStore.startPolling = () => {};
memoryStore.stopPolling = () => {};
memoryStore.refreshDisk = async () => {};
systemMetricsStore.startPolling = () => {};
systemMetricsStore.stopPolling = () => {};
awakeStore.refresh = async () => {};
scanStore.init = async () => {};
scanStore.isStale = () => false;
scanStore.observeFreshness = () => () => {};
scanStore.cancelScan = async () => { scanStore.isCancelling = true; };
usageStore.refreshIfStale = async () => {};
let subscribers = 0;
usageStore.observeAutoRefresh = () => { subscribers += 1; return () => { subscribers -= 1; }; };
const fresh = await mockApi.getAiUsage();
fresh.providers = fresh.providers.filter(provider => ['codex', 'antigravity'].includes(provider.id));
const inventory = (await mockApi.startScan(() => {})).result;
usageStore.snapshot = null;
usageStore.isLoading = true;
usageStore.loadingProviders = ['codex', 'antigravity'];
mount(QuickPanel, { target: document.getElementById('app')! });
// The Tauri marker above exercises the mocked window lifecycle only. The
// validation build aliases all Tauri APIs to quickWindowMock, whose unknown
// commands fail closed; scan publication below is also a fixture-only port.
const context = document.createElement('p');
context.dataset.validationContext = 'synthetic';
context.textContent = 'Browser fixture · synthetic data · mocked window and IPC';
context.style.cssText = 'margin:6px 8px;font:11px/16px system-ui;color:#334155';
document.body.append(context);

// Wait through both the fit debounce and its ResizeObserver acknowledgement.
const settle = async () => { await tick(); await new Promise(resolve => setTimeout(resolve, 420)); };
function measurement() {
  const app = document.getElementById('app')!;
  const shell = app.firstElementChild! as HTMLElement;
  const footer = shell.lastElementChild! as HTMLElement;
  const scroller = shell.children[1] as HTMLElement;
  return {
    width: app.clientWidth, height: app.clientHeight,
    contentHeight: document.querySelector('.quick-panel-content')!.scrollHeight,
    gaugeSlots: document.querySelectorAll('.quick-ai-gauge-slot').length,
    meters: document.querySelectorAll('[role="meter"]').length,
    footerBottom: Math.round(footer.getBoundingClientRect().bottom),
    scrollable: scroller.scrollHeight > scroller.clientHeight,
    subscribers, visible: windowProbe.visible,
    requests: windowProbe.requests.map(request => ({ ...request })),
  };
}
async function transition(kind: string) {
  const snapshot = structuredClone(fresh);
  snapshot.fetched_at = Math.floor(Date.now() / 1000) - (kind === 'stale' ? 600 : 0);
  usageStore.isLoading = kind === 'loading' || kind === 'partial';
  usageStore.loadingProviders = kind === 'loading' ? ['codex', 'antigravity'] : kind === 'partial' ? ['antigravity'] : [];
  if (kind === 'empty') snapshot.providers = [];
  if (kind === 'partial') snapshot.providers = snapshot.providers.slice(0, 1);
  for (const provider of snapshot.providers) {
    provider.collection_status = kind === 'error' ? 'protocol_error' : kind === 'timeout' ? 'timeout' : 'fresh';
    if (kind === 'error' || kind === 'timeout') provider.status_message = 'Usage could not be refreshed. Try again.';
    if (kind === 'disconnected') { provider.connected = false; provider.windows = []; }
    if (kind === 'long') { provider.name = `${provider.name} development account with a long organization and workspace name requiring more than one line`; provider.collection_status = 'unavailable'; provider.status_message = 'Account connection is unavailable; open AI Activity to review the configured provider and its authentication status.'; provider.windows = []; }
  }
  usageStore.snapshot = snapshot;
  await settle();
  return measurement();
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
  if (kind === 'stopped') {
    result.quality = 'partial';
    result.cancelled = true;
    result.gaps = [{ kind: 'permission_denied', count: 3 }];
  }
  // Preserve production acceptance/invalidation through runScan, with a typed
  // publication port that never calls a native command in this browser fixture.
  nativeApi.startScan = async () => ({ result, discovery: { status: 'exhausted' } });
  await scanStore.runScan();
  if (kind === 'initial') {
    scanStore.lastScan = null;
    scanStore.selectedMap = {};
  }
  if (kind === 'preparing' || kind === 'scanning' || kind === 'refreshing' || kind === 'stopping') {
    scanStore.isScanning = true;
    scanStore.scanId = kind === 'preparing' ? null : 'synthetic-quick-validation-scan';
    scanStore.scanStartedAt = kind === 'preparing' ? Date.now() : Date.now() - 71_000;
    scanStore.currentRoot = kind === 'preparing' ? null : { name: 'npm Cache', path: '/fixture/Library/Caches/npm' };
    scanStore.foundItemCount = kind === 'preparing' ? 0 : 53;
    scanStore.isRefreshingAfterClean = kind === 'refreshing';
    scanStore.isCancelling = kind === 'stopping';
  }
  if (kind === 'cleaning') {
    scanStore.isCleaning = true;
    scanStore.cleanProgress = { currentItem: 'Verified cache', index: 2, total: 8, percent: 25 };
  }
  if (kind === 'failed') {
    nativeApi.startScan = async () => { throw new Error('The storage check did not complete. Try scanning again.'); };
    await scanStore.runScan();
  }
  await settle();
  return measurement();
}
const driver = {
  ready: false,
  transition,
  scan,
  measurement,
  async metrics(cpu = 72, memory = 12.3) {
    if (systemMetricsStore.cpu) systemMetricsStore.cpu.usage_percent = cpu;
    if (memoryStore.memory) memoryStore.memory.used_bytes = memory * 1024 ** 3;
    await tick();
    return measurement();
  },
  async theme(dark: boolean) {
    settingsStore.settings.theme = dark ? 'dark' : 'light';
    document.documentElement.classList.toggle('dark', dark);
    await settle();
    return measurement();
  },
  async visibility(visible: boolean) { windowProbe.setVisible(visible); await settle(); return measurement(); },
  async viewport(width: number, maximum = 740) { windowProbe.viewport(width, maximum); await settle(); return measurement(); },
  async preferences() { settingsStore.settings.quick_panel_sections = ['cpu', 'memory', 'storage', 'agent_activity']; await settle(); return measurement(); },
  async delayedMonitor() {
    let resolve!: (value: { scaleFactor: number; workArea: { size: { height: number } } }) => void;
    windowProbe.deferred = new Promise(done => { resolve = done; });
    windowProbe.viewport(360, 740);
    await new Promise(done => setTimeout(done, 220));
    windowProbe.setVisible(false);
    windowProbe.setVisible(true);
    await settle();
    const before = windowProbe.requests.length;
    resolve({ scaleFactor: 2, workArea: { size: { height: 648 } } });
    await settle();
    return { before, after: windowProbe.requests.length, measurement: measurement() };
  },
};
(window as unknown as { quickPanelValidation: typeof driver }).quickPanelValidation = driver;
await settle();
driver.ready = true;
