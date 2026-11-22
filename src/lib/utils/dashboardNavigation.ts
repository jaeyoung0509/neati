/**
 * Navigation model (#450): four sections plus Settings. A section owns one or
 * more routes, shown as tabs inside it. Route ids are unchanged, so the tray,
 * the Quick Panel and saved deep links keep working.
 */
export type SectionId = 'home' | 'clean' | 'system' | 'ai' | 'settings';

export interface SectionRoute {
  /** The route a section tab opens. */
  route: string;
  label: string;
}

export interface SectionDef {
  id: SectionId;
  label: string;
  routes: readonly SectionRoute[];
}

export const SECTIONS: readonly SectionDef[] = [
  { id: 'home', label: 'Home', routes: [{ route: 'overview', label: 'Overview' }] },
  {
    id: 'clean',
    label: 'Clean',
    routes: [
      { route: 'storage', label: 'Caches' },
      { route: 'developer-artifacts', label: 'Projects' },
      { route: 'large-files', label: 'Large Files' },
      { route: 'applications', label: 'Apps' },
      { route: 'docker', label: 'Containers' },
      { route: 'models', label: 'Models' },
      { route: 'disks', label: 'Disks' },
    ],
  },
  {
    id: 'system',
    label: 'System',
    routes: [
      { route: 'performance', label: 'Performance' },
      { route: 'development_servers', label: 'Ports' },
      { route: 'awake', label: 'Keep Awake' },
    ],
  },
  {
    id: 'ai',
    label: 'AI',
    routes: [
      { route: 'projects', label: 'Activity' },
      { route: 'ai_control', label: 'Controls' },
    ],
  },
  { id: 'settings', label: 'Settings', routes: [{ route: 'settings', label: 'Settings' }] },
];

/** Detail routes that live inside a section tab's view. */
const ROUTE_TAB_OWNER: Readonly<Record<string, string>> = {
  memory: 'performance',
  cpu: 'performance',
  battery: 'performance',
  usage: 'projects',
};

/** The section tab that shows `route`, after legacy ids are normalized. */
export function sectionTabForRoute(route: string): string {
  if (route === 'memory') return 'performance';
  const normalized = normalizeDashboardTab(route);
  return ROUTE_TAB_OWNER[normalized] ?? normalized;
}

export function sectionForRoute(route: string): SectionDef {
  const tab = sectionTabForRoute(route);
  return SECTIONS.find((section) => section.routes.some((entry) => entry.route === tab)) ?? SECTIONS[0];
}

/**
 * Where a section opens: the route last shown there when it is still
 * available, otherwise its first available tab.
 */
export function sectionEntryRoute(
  section: SectionDef,
  lastRoute: string | undefined,
  isAvailable: (route: string) => boolean,
): string | null {
  if (lastRoute && sectionForRoute(lastRoute).id === section.id && isAvailable(lastRoute)) return lastRoute;
  return section.routes.find((entry) => isAvailable(entry.route))?.route ?? null;
}

export type DashboardStage = 'core' | 'beta';

/**
 * v1 is storage cleanup, the readings that explain it, and Keep Awake. The
 * destinations below work, but their data is advisory or depends on tools
 * neati only observes, so they are labelled Beta rather than hidden.
 */
const BETA_ROUTES: ReadonlySet<string> = new Set([
  'projects',
  'usage',
  'ai_control',
  'docker',
  'models',
  'development_servers',
]);

export const BETA_NOTE = 'Beta: this view works, but its data depends on other tools and may be incomplete.';

export function dashboardStage(tab: string): DashboardStage {
  return BETA_ROUTES.has(normalizeDashboardTab(tab)) ? 'beta' : 'core';
}

/**
 * Normalizes a persisted dashboard tab id. `disk` predates the Disks sub-tab
 * and `memory` predates the Performance page, so both keep working for saved
 * settings and deep links.
 */
export function normalizeDashboardTab(tab: string): string {
  if (tab === 'disk') return 'disks';
  if (tab === 'memory') return 'performance';
  return tab;
}

/** Cleanup first; otherwise the first route this platform can open. */
export function initialDashboardRoute(isAvailable: (route: string) => boolean): string {
  if (isAvailable('storage')) return 'storage';
  for (const section of SECTIONS) {
    const route = section.routes.find((entry) => isAvailable(entry.route))?.route;
    if (route) return route;
  }
  return 'settings';
}

export function dashboardNavigationOwner(tab: string): string {
  const route = normalizeDashboardTab(tab);
  if (['large-files', 'applications', 'developer-artifacts', 'disks'].includes(route)) return 'storage';
  if (['memory', 'cpu', 'battery'].includes(tab)) return 'performance';
  if (['usage', 'ai_control'].includes(route)) return 'projects';
  return route;
}
