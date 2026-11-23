import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SettingsCleanupRules from '../lib/components/settings/SettingsCleanupRules.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import {
  DEFAULT_CLEANUP_RULES,
  cleanInclusionDetail,
  cleanInclusionLabel,
  idleDays,
  resolveCleanupRules,
} from '../lib/utils/cleanupRules';
import type { CleanInclusion } from '../lib/models/types';

const INCLUSIONS: CleanInclusion[] = [
  'included',
  'not_offered',
  'not_verified',
  'rule_off',
  'no_lockfile',
  'recently_active',
  'activity_unknown',
  'in_use',
  'use_unknown',
];

describe('Project cleanup rules (#504)', () => {
  const original = settingsStore.settings.cleanup_rules;
  afterEach(() => {
    settingsStore.settings.cleanup_rules = original;
  });

  it('fills an older settings file with the backend defaults, which keep new rules off', () => {
    expect(resolveCleanupRules(undefined)).toEqual(DEFAULT_CLEANUP_RULES);
    expect(resolveCleanupRules({})).toEqual(DEFAULT_CLEANUP_RULES);
    expect(resolveCleanupRules({ dependency_stores: { enabled: true } })).toEqual({
      build_output_min_idle_days: 0,
      dependency_stores: { enabled: true, idle_days: 30 },
      worktrees: { enabled: false, idle_days: 14 },
    });
    expect(DEFAULT_CLEANUP_RULES.dependency_stores.enabled).toBe(false);
  });

  it('names every inclusion state with a short label and a reason', () => {
    for (const inclusion of INCLUSIONS) {
      expect(cleanInclusionLabel(inclusion).length).toBeGreaterThan(0);
      expect(cleanInclusionDetail(inclusion).length).toBeGreaterThan(0);
    }
    expect(cleanInclusionLabel('included')).toBe('In Clean');
    expect(cleanInclusionDetail('rule_off')).toContain('Settings');
  });

  it('counts idle days from the backend timestamp', () => {
    const now = Date.UTC(2026, 10, 24);
    expect(idleDays(null, now)).toBeNull();
    expect(idleDays(now / 1000 - 45 * 86_400, now)).toBe(45);
    expect(idleDays(now / 1000 + 60, now)).toBe(0);
  });

  it('shows the dependency rule off by default and states the consequence', () => {
    settingsStore.settings.cleanup_rules = {};
    const body = render(SettingsCleanupRules).body;
    expect(body).toContain('Project cleanup rules');
    expect(body).toContain('permanently removes');
    expect(body).toContain('aria-label="Dependency folders of inactive projects"');
    expect(body).toContain('lockfile');
    // The period picker appears only once the rule is on.
    expect(body).not.toContain('cleanup-dependency-days');
    expect(body).toContain('Whenever nothing has it open');
  });

  it('offers the period once the dependency rule is on', () => {
    settingsStore.settings.cleanup_rules = {
      build_output_min_idle_days: 14,
      dependency_stores: { enabled: true, idle_days: 60 },
      worktrees: { enabled: false, idle_days: 14 },
    };
    const body = render(SettingsCleanupRules).body;
    expect(body).toContain('cleanup-dependency-days');
    expect(body).toContain('60 days without work');
  });
});
