import type { DashboardTab } from '../models/types';

/**
 * Persisted sidebar order. Direct workflow shortcuts follow their owning
 * destination without rewriting the user's preferences.
 */
export const DEFAULT_DASHBOARD_TABS: DashboardTab[] = [
  'overview',
  'storage',
  'performance',
  'projects',
  'docker',
  'models',
  'development_servers',
  'awake',
];

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

export function dashboardGroup(tab: string): string | null {
  if (tab === 'overview') return null;
  if (tab === 'storage' || tab === 'disk') return 'Storage';
  if (tab === 'performance' || tab === 'memory') return 'System';
  return 'Tools';
}

/** Saved visibility and relative order within each task group remain intact. */
export function groupedDashboardTabs(tabs: readonly DashboardTab[]): DashboardTab[] {
  return [null, 'Storage', 'System', 'Tools'].flatMap(group =>
    tabs.filter(tab => dashboardGroup(tab) === group)
  );
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

export function initialDashboardTab(
  tabs: readonly DashboardTab[],
  isAvailable: (tab: string) => boolean,
): string {
  if (tabs.includes('storage') && isAvailable('storage')) return 'storage';
  return normalizeDashboardTab(tabs.find(isAvailable) ?? 'settings');
}

export function dashboardNavigationOwner(tab: string): string {
  const route = normalizeDashboardTab(tab);
  if (['large-files', 'applications', 'developer-artifacts', 'disks'].includes(route)) return 'storage';
  if (['memory', 'cpu', 'battery'].includes(tab)) return 'performance';
  if (['usage', 'ai_control'].includes(route)) return 'projects';
  return route;
}
