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
import { SECTIONS, dashboardNavigationOwner, initialDashboardRoute, sectionEntryRoute, sectionForRoute, sectionTabForRoute } from '../lib/utils/dashboardNavigation';

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

  it('keeps Development Servers out of the Memory panel', () => {
    const memorySource = readFileSync(
      new URL('../lib/components/performance/MemoryPanel.svelte', import.meta.url),
      'utf8'
    );
    expect(memorySource).not.toContain('developmentPortsStore');
    expect(memorySource).not.toContain('Development Servers Section');
  });

  it('shows four sections plus Settings instead of one entry per feature', () => {
    const rendered = render(Dashboard);
    const sections = [...rendered.body.matchAll(/data-section="([a-z]+)"/g)].map((match) => match[1]);
    expect(sections).toEqual(['home', 'clean', 'system', 'ai']);
    expect(rendered.body).toContain('aria-label="Settings"');
    // Old per-feature sidebar entries and group headings are gone.
    for (const label of ['Containers', 'Local Models', 'Dev Servers', 'Keep Awake', 'Memory']) {
      expect(rendered.body).not.toContain(`aria-label="${label}"`);
    }
    expect(rendered.body).not.toContain('data-navigation-parent');
    expect(rendered.body).not.toContain('>Tools</div>');
  });

  it('titles the open view with the name its section tab uses', () => {
    const rendered = render(Dashboard);
    // The app opens on Clean → Caches.
    expect(rendered.body).toMatch(/<h1[^>]*>Caches<\/h1>/);
    expect(rendered.body).not.toMatch(/<h1[^>]*>Cleanup<\/h1>/);
  });

  it('keeps one name per view across tabs and page titles', () => {
    const titles: Record<string, [string, RegExp]> = {
      Projects: ['DeveloperArtifactsView', />Projects<\/h1>/],
      Apps: ['ApplicationsView', />Apps<\/h1>/],
      Models: ['ModelsView', /title="Models"/],
      Disks: ['DiskView', /title="Disks"/],
      Ports: ['DevelopmentServersView', /title="Ports"/],
      Activity: ['ProjectCockpitView', /title="Activity"/],
      Controls: ['AiControlCenterView', /title="Controls"/],
    };
    const tabLabels = SECTIONS.flatMap((section) => section.routes.map((entry) => entry.label));
    for (const [label, [view, pattern]] of Object.entries(titles)) {
      expect(tabLabels, label).toContain(label);
      const source = readFileSync(new URL(`../routes/dashboard/${view}.svelte`, import.meta.url), 'utf8');
      expect(source, view).toMatch(pattern);
    }
  });

  it('carries the reclaimable total on the Clean section', () => {
    const rendered = render(Dashboard);
    expect(rendered.body).toMatch(/aria-label="Clean, 14 MB reclaimable"/);
  });

  it('opens on Clean with its views as tabs, Beta labelled where data is advisory', () => {
    const rendered = render(Dashboard);
    expect(rendered.body).toContain('aria-label="Clean views"');
    const tablist = rendered.body.split('aria-label="Clean views"')[1]?.split('</div>')[0] ?? '';
    for (const label of ['Caches', 'Projects', 'Large Files', 'Apps', 'Containers', 'Models', 'Disks']) {
      expect(tablist).toContain(`<span>${label}</span>`);
    }
    expect(tablist.match(/>\s*Beta\s*</g)).toHaveLength(2);
  });
});

describe('Cleanup-first navigation', () => {
  it('keeps a closed view visible as a disabled tab that names the reason', () => {
    platformCapabilitiesStore.capabilities = {
      ...goldenCapabilitiesByPlatform.macos,
      developer_artifacts: {
        status: 'unavailable',
        reason: 'Developer artifact cleanup is unavailable on this platform.',
      },
    };

    const rendered = render(Dashboard);
    expect(rendered.body).toMatch(
      /<button(?=[^>]*disabled)(?=[^>]*role="tab")(?=[^>]*title="Developer artifact cleanup is unavailable on this platform\.")[^>]*>/
    );
  });

  it('closes the Apps tab only when installed apps cannot be inspected', () => {
    const appsTab = (body: string) => body.match(/<button[^>]*role="tab"[^>]*>(?:(?!<\/button>).)*<span>Apps<\/span>/s)?.[0] ?? '';
    platformCapabilitiesStore.capabilities = {
      ...goldenCapabilitiesByPlatform.macos,
      installed_apps: { status: 'unavailable', reason: 'Application inventory is not supported here.' },
    };
    const closed = appsTab(render(Dashboard).body);
    expect(closed).toContain('disabled');
    expect(closed).toContain('Application inventory is not supported here.');

    for (const installed_apps of [{ status: 'available' as const }, { status: 'read_only' as const, reason: 'Inspection only' }]) {
      platformCapabilitiesStore.capabilities = { ...goldenCapabilitiesByPlatform.macos, installed_apps };
      const open = appsTab(render(Dashboard).body);
      expect(open).toContain('<span>Apps</span>');
      expect(open).not.toMatch(/\sdisabled/);
    }
  });

  it('opens Memory as a focused page without another section selector', () => {
    const rendered = render(PerformanceView, { props: { initialTab: 'memory', standaloneMemory: true } });
    expect(rendered.body).toContain('Memory readings');
    expect(rendered.body).not.toContain('aria-label="Performance sections"');
    expect(rendered.body).not.toContain('role="tablist"');
    expect(rendered.body).not.toContain('>Performance</h1>');
  });
  it('opens cleanup first, then the first route the platform can open', () => {
    expect(initialDashboardRoute(() => true)).toBe('storage');
    expect(initialDashboardRoute((route) => route !== 'storage')).toBe('overview');
    expect(initialDashboardRoute((route) => route === 'awake')).toBe('awake');
    expect(initialDashboardRoute(() => false)).toBe('settings');
  });

  it('maps every legacy and detail route to one section and tab', () => {
    const cases: Array<[string, string, string]> = [
      ['overview', 'home', 'overview'],
      ['storage', 'clean', 'storage'],
      ['disk', 'clean', 'disks'],
      ['disks', 'clean', 'disks'],
      ['large-files', 'clean', 'large-files'],
      ['applications', 'clean', 'applications'],
      ['developer-artifacts', 'clean', 'developer-artifacts'],
      ['docker', 'clean', 'docker'],
      ['models', 'clean', 'models'],
      ['performance', 'system', 'performance'],
      ['memory', 'system', 'performance'],
      ['cpu', 'system', 'performance'],
      ['battery', 'system', 'performance'],
      ['development_servers', 'system', 'development_servers'],
      ['awake', 'system', 'awake'],
      ['projects', 'ai', 'projects'],
      ['usage', 'ai', 'projects'],
      ['ai_control', 'ai', 'ai_control'],
      ['settings', 'settings', 'settings'],
    ];
    for (const [route, section, tab] of cases) {
      expect(sectionForRoute(route).id, route).toBe(section);
      expect(sectionTabForRoute(route), route).toBe(tab);
    }
  });

  it('returns to the route a section last showed, when it is still available', () => {
    const clean = SECTIONS.find((section) => section.id === 'clean')!;
    expect(sectionEntryRoute(clean, undefined, () => true)).toBe('storage');
    expect(sectionEntryRoute(clean, 'models', () => true)).toBe('models');
    expect(sectionEntryRoute(clean, 'models', (route) => route !== 'models')).toBe('storage');
    // A route remembered for another section is not this section's entry.
    expect(sectionEntryRoute(clean, 'awake', () => true)).toBe('storage');
    expect(sectionEntryRoute(clean, undefined, () => false)).toBeNull();
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
