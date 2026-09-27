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
