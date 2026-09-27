import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import Dashboard from '../routes/dashboard/Dashboard.svelte';
import PerformanceView from '../routes/dashboard/PerformanceView.svelte';
import { scanStore } from '../lib/stores/scan.svelte';
import { platformCapabilitiesStore } from '../lib/stores/platformCapabilities.svelte';
import { platformContextStore } from '../lib/stores/platformContext.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { goldenCapabilitiesByPlatform } from '../lib/models/platformCapabilities';
import { mockApi } from '../lib/api/mock';
import { DEFAULT_DASHBOARD_TABS, dashboardNavigationOwner, initialDashboardTab } from '../lib/utils/dashboardNavigation';

beforeEach(() => {
  scanStore.lastScan = {
    scan_id: 'sidebar-test',
    valid_for_seconds: 300,
    started_at: Date.now() - 1000,
    finished_at: Date.now(),
    categories: [
      {
        category: 'ai',
        display_name: 'AI Tools',
        items: [
          {
            id: 'sidebar-safe',
            signature_id: 'sidebar.safe',
            name: 'Sidebar test cache',
            category: 'ai',
            risk: 'safe',
            path: '/tmp/sidebar-test',
            size: { logical: 14 * 1024 * 1024, allocated: 14 * 1024 * 1024 },
            file_count: 1,
            description: 'Dashboard sidebar fixture',
            is_selected: true,
            last_modified: Date.now(),
            exists: true,
            quality: 'fresh',
            disposition: {
              eligibility: 'auto_cleanable',
              reason: null,
              cleanable_bytes: 14 * 1024 * 1024,
            },
          },
        ],
        total_bytes: 14 * 1024 * 1024,
        safe_bytes: 14 * 1024 * 1024,
        rebuild_bytes: 0,
        manual_bytes: 0,
      },
    ],
    total_bytes: 14 * 1024 * 1024,
    safe_bytes: 14 * 1024 * 1024,
    rebuild_bytes: 0,
    manual_bytes: 0,
  };
  scanStore.selectedMap = { 'sidebar-safe': true };
});

afterEach(() => {
  scanStore.lastScan = null;
  scanStore.selectedMap = {};
  platformCapabilitiesStore.reset();
  platformContextStore.reset();
});

describe('Dashboard sidebar affordances', () => {
  it('keeps the collapse control labelled and the Storage status visually quiet', () => {
    const rendered = render(Dashboard);

    expect(rendered.body).toContain('aria-label="Collapse sidebar"');
    expect(rendered.body).toContain('title="Collapse sidebar"');
    expect(rendered.body).toContain('rounded-md border border-transparent');
    expect(rendered.body).toContain('text-caption font-mono font-medium tracking-tight text-primary');
    expect(rendered.body).toContain('14 MB');
  });

  it('exposes Development Servers as its own dashboard route', () => {
    const rendered = render(Dashboard);
    const memorySource = readFileSync(
      new URL('../lib/components/performance/MemoryPanel.svelte', import.meta.url),
      'utf8'
    );

    expect(rendered.body).toContain('Dev Servers');
    expect(memorySource).not.toContain('developmentPortsStore');
    expect(memorySource).not.toContain('Development Servers Section');
  });

  it('exposes the consolidated AI Activity dashboard route', () => {
    const rendered = render(Dashboard);
    expect(rendered.body).toContain('AI Activity');
  });

  it('keeps tools visible with one heading and a direct Memory shortcut', () => {
    const previousTabs = settingsStore.settings.dashboard_tabs;
    settingsStore.settings = {
      ...settingsStore.settings,
      dashboard_tabs: [
        'overview', 'docker', 'storage', 'models', 'performance',
        'development_servers', 'projects', 'awake',
      ],
    };

    try {
      const rendered = render(Dashboard);
      expect(rendered.body.match(/>Tools<\/div>/g)).toHaveLength(1);
      expect(rendered.body).not.toContain('aria-label="Tools"');
      expect(rendered.body).toContain('aria-label="Memory"');
      expect(rendered.body).toContain('aria-label="Developer Artifacts"');
      expect(rendered.body).toContain('aria-label="Large Files"');
      expect(rendered.body).toContain('aria-label="Applications"');
      expect(settingsStore.settings.dashboard_tabs[0]).toBe('overview');
      expect(rendered.body.indexOf('aria-label="Containers"')).toBeLessThan(rendered.body.indexOf('aria-label="Local Models"'));
    } finally {
      settingsStore.settings = { ...settingsStore.settings, dashboard_tabs: previousTabs };
    }
  });
});

describe('Cleanup-first navigation', () => {
  it('hides direct shortcuts when their owning destinations are disabled', () => {
    const previousTabs = settingsStore.settings.dashboard_tabs;
    settingsStore.settings = { ...settingsStore.settings, dashboard_tabs: ['overview', 'projects'] };
    try {
      const rendered = render(Dashboard);
      for (const label of ['Memory', 'Developer Artifacts', 'Large Files', 'Applications']) {
        expect(rendered.body).not.toContain(`aria-label="${label}"`);
      }
    } finally {
      settingsStore.settings = { ...settingsStore.settings, dashboard_tabs: previousTabs };
    }
  });

  it('gates Developer Artifacts with its dedicated platform capability', () => {
    platformCapabilitiesStore.capabilities = {
      ...goldenCapabilitiesByPlatform.macos,
      developer_artifacts: {
        status: 'unavailable',
        reason: 'Developer artifact cleanup is unavailable on this platform.',
      },
    };

    const rendered = render(Dashboard);
    expect(rendered.body).toMatch(
      /<button(?=[^>]*disabled)(?=[^>]*aria-label="Developer Artifacts")(?=[^>]*title="Developer artifact cleanup is unavailable on this platform\.")[^>]*>/
    );
  });

  it('opens Memory as a focused page without another section selector', () => {
    const rendered = render(PerformanceView, { props: { initialTab: 'memory', standaloneMemory: true } });
    expect(rendered.body).toContain('Memory readings');
    expect(rendered.body).not.toContain('aria-label="Performance sections"');
    expect(rendered.body).not.toContain('role="tablist"');
    expect(rendered.body).not.toContain('>Performance</h1>');
  });
  it('opens enabled cleanup ahead of overview without changing the saved order', () => {
    const tabs = [...DEFAULT_DASHBOARD_TABS];
    expect(initialDashboardTab(tabs, () => true)).toBe('storage');
    expect(tabs).toEqual(DEFAULT_DASHBOARD_TABS);
  });

  it('respects hidden and unavailable destinations, including a capability failure', () => {
    expect(initialDashboardTab(['models', 'performance'], () => true)).toBe('models');
    expect(initialDashboardTab(['storage', 'performance'], tab => tab !== 'storage')).toBe('performance');
    expect(initialDashboardTab(DEFAULT_DASHBOARD_TABS, () => false)).toBe('settings');
    expect(initialDashboardTab([], () => true)).toBe('settings');
  });

  it('keeps detail routes associated with their visible navigation destination', () => {
    for (const tab of ['disks', 'disk', 'applications', 'large-files', 'developer-artifacts']) {
      expect(dashboardNavigationOwner(tab)).toBe('storage');
    }
    for (const tab of ['cpu', 'battery', 'memory']) expect(dashboardNavigationOwner(tab)).toBe('performance');
    for (const tab of ['projects', 'ai_control', 'usage']) expect(dashboardNavigationOwner(tab)).toBe('projects');
  });
});

describe('Dashboard platform chrome', () => {
  it('reserves the overlay top band only when the backend reports one', async () => {
    platformCapabilitiesStore.capabilities = goldenCapabilitiesByPlatform.macos;

    platformContextStore.context = await mockApi.getPlatformContext();
    const overlay = render(Dashboard);
    expect(overlay.body).toContain('titlebar-drag-region absolute top-0 left-0 right-0 h-7 z-30');
    expect(overlay.body).toContain('pt-9');
    expect(overlay.body).toContain('pt-10');

    platformContextStore.context = {
      ...(await mockApi.getPlatformContext()),
      platform: 'windows',
      overlay_title_bar: false,
      native_caption_bar: true,
    };
    const nativeCaption = render(Dashboard);
    // A native caption bar must neither reserve the band nor expose a drag strip.
    expect(nativeCaption.body).not.toContain('titlebar-drag-region');
    expect(nativeCaption.body).not.toContain('pt-9');
    expect(nativeCaption.body).not.toContain('pt-10');
  });

  it('renders a retryable capability failure instead of an unsupported platform', () => {
    platformCapabilitiesStore.capabilities = null;
    platformCapabilitiesStore.error = 'IPC unavailable';

    const failed = render(Dashboard);

    expect(failed.body).toContain('Platform capabilities unavailable');
    expect(failed.body).toContain('IPC unavailable');
    expect(failed.body).toContain('Retry');
    expect(failed.body).not.toContain('Loading platform capabilities');

    platformCapabilitiesStore.capabilities = goldenCapabilitiesByPlatform.windows;
    platformCapabilitiesStore.error = null;

    const unsupported = render(Dashboard);

    expect(unsupported.body).not.toContain('Platform capabilities unavailable');
    expect(unsupported.body).not.toContain('Loading platform capabilities');
  });
});
