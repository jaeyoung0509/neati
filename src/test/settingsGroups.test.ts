import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SettingsView from '../routes/dashboard/SettingsView.svelte';
import { SETTINGS_GROUPS, settingsGroupStore, type SettingsGroup } from '../lib/stores/settingsGroup.svelte';

/** A heading or landmark text every group must show, and only that group. */
const MARKERS: Record<SettingsGroup, string[]> = {
  cleanup: ['Project cleanup rules', 'Automatic Clean', 'Quick Clean categories', 'Cache scanning'],
  agents: ['Command line and agents', 'AI tool profiles'],
  app: ['Appearance', 'Updates', 'About neati'],
};

describe('Settings in three groups (#519)', () => {
  afterEach(() => {
    settingsGroupStore.current = 'cleanup';
  });

  it('offers the groups as keyboard tabs and opens on Cleanup', () => {
    expect(SETTINGS_GROUPS.map((group) => group.id)).toEqual(['cleanup', 'agents', 'app']);
    const body = render(SettingsView).body;
    expect(body).toContain('aria-label="Settings groups"');
    expect(body.match(/role="tab"/g)?.length).toBe(3);
  });

  it('shows each setting in exactly one group', () => {
    for (const group of SETTINGS_GROUPS) {
      const body = render(SettingsView, { props: { group: group.id } }).body;
      for (const [owner, markers] of Object.entries(MARKERS)) {
        for (const marker of markers) {
          if (owner === group.id) {
            expect(body, `${marker} in ${group.id}`).toContain(marker);
          } else {
            expect(body, `${marker} not in ${group.id}`).not.toContain(`>${marker}<`);
          }
        }
      }
    }
  });
});
