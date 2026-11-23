import type { CleanInclusion, CleanupRules } from '../models/types';

/** Cleanup rules with every field present, as the backend sanitizes them. */
export type ResolvedCleanupRules = {
  build_output_min_idle_days: number;
  dependency_stores: { enabled: boolean; idle_days: number };
  worktrees: { enabled: boolean; idle_days: number };
};

/** The backend's defaults (#504): new removal rules start off. */
export const DEFAULT_CLEANUP_RULES: ResolvedCleanupRules = {
  build_output_min_idle_days: 0,
  dependency_stores: { enabled: false, idle_days: 30 },
  worktrees: { enabled: false, idle_days: 14 },
};

/** Fills fields an older settings file leaves out with the defaults. */
export function resolveCleanupRules(rules: CleanupRules | null | undefined): ResolvedCleanupRules {
  return {
    build_output_min_idle_days:
      rules?.build_output_min_idle_days ?? DEFAULT_CLEANUP_RULES.build_output_min_idle_days,
    dependency_stores: {
      enabled: rules?.dependency_stores?.enabled ?? DEFAULT_CLEANUP_RULES.dependency_stores.enabled,
      idle_days: rules?.dependency_stores?.idle_days ?? DEFAULT_CLEANUP_RULES.dependency_stores.idle_days,
    },
    worktrees: {
      enabled: rules?.worktrees?.enabled ?? DEFAULT_CLEANUP_RULES.worktrees.enabled,
      idle_days: rules?.worktrees?.idle_days ?? DEFAULT_CLEANUP_RULES.worktrees.idle_days,
    },
  };
}

/** The bounds the backend clamps to; the inputs show the same range. */
export const MIN_DEPENDENCY_IDLE_DAYS = 7;
export const MAX_RULE_IDLE_DAYS = 365;

/** Why an item is in one-button Clean or left for review, in a few words. */
export function cleanInclusionLabel(inclusion: CleanInclusion): string {
  switch (inclusion) {
    case 'included':
      return 'In Clean';
    case 'not_offered':
      return 'Review only';
    case 'not_verified':
      return 'Not verified';
    case 'rule_off':
      return 'Dependency rule off';
    case 'no_lockfile':
      return 'No lockfile';
    case 'recently_active':
      return 'Recently worked on';
    case 'activity_unknown':
      return 'Activity unknown';
    case 'in_use':
      return 'In use';
    case 'use_unknown':
      return 'Use unknown';
    default:
      return 'Review only';
  }
}

/** The longer explanation shown as a tooltip or detail line. */
export function cleanInclusionDetail(inclusion: CleanInclusion): string {
  switch (inclusion) {
    case 'included':
      return 'One-button Clean permanently removes this. The project recreates it when needed.';
    case 'not_offered':
      return 'This kind is always reviewed here before removal.';
    case 'not_verified':
      return 'Its size or generated-only ownership was not fully verified, so Clean leaves it.';
    case 'rule_off':
      return 'Turn on "Dependency folders of inactive projects" in Settings to include it in Clean.';
    case 'no_lockfile':
      return 'No lockfile sits beside it, so a reinstall might not reproduce it. Clean leaves it.';
    case 'recently_active':
      return 'The project was worked on within your rule’s period, or a running process works inside it.';
    case 'activity_unknown':
      return 'When the project was last worked on could not be observed, so Clean leaves it.';
    case 'in_use':
      return 'A running process has files open in it.';
    case 'use_unknown':
      return 'neati could not confirm that nothing is using it.';
    default:
      return 'This kind is always reviewed here before removal.';
  }
}

/** Whole days since an epoch-seconds timestamp, never negative. */
export function idleDays(lastActivity: number | null, nowMs: number = Date.now()): number | null {
  if (lastActivity == null) return null;
  return Math.max(0, Math.floor((nowMs / 1000 - lastActivity) / 86_400));
}
