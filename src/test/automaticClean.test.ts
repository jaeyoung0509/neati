import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SettingsAutomaticClean from '../lib/components/settings/SettingsAutomaticClean.svelte';
import { automaticCleanSummary } from '../lib/utils/automaticClean';
import type { AutomaticCleanStatus } from '../lib/models/types';

const NOW = Date.now();
const now = Math.floor(NOW / 1000);

function status(partial: Partial<AutomaticCleanStatus>): AutomaticCleanStatus {
  return {
    settings: { enabled: true, interval: 'weekly' },
    last_run: null,
    next_due_at: null,
    waiting: null,
    available: true,
    ...partial,
  };
}

describe('Automatic Clean (#507)', () => {
  it('says what the last run did and when the next one may start', () => {
    expect(
      automaticCleanSummary(
        status({
          last_run: { at: now - 2 * 86_400, reclaimed_bytes: 1024 * 1024 * 1024, removed: 3, failed: 1, error: null },
          next_due_at: now + 5 * 86_400,
          waiting: 'not_due',
        }),
        NOW
      )
    ).toBe('Last run 2 days ago removed 3 items (1 GB); 1 could not be removed. Next run in about 5 days.');
    expect(automaticCleanSummary(status({ next_due_at: 0, waiting: 'on_battery' }), NOW)).toBe(
      'Has not run yet. Waiting for AC power.'
    );
    expect(
      automaticCleanSummary(
        status({ last_run: { at: now - 3600, reclaimed_bytes: 0, removed: 0, failed: 0, error: 'The scan is busy.' } }),
        NOW
      )
    ).toContain('stopped: The scan is busy.');
    expect(automaticCleanSummary(status({ available: false }), NOW)).toContain('macOS only');
    expect(
      automaticCleanSummary(status({ settings: { enabled: false, interval: 'weekly' }, waiting: 'disabled' }), NOW)
    ).toBe('Off.');
  });

  it('states the conditions and the permanence, and starts off', () => {
    const body = render(SettingsAutomaticClean, {
      props: { initialStatus: status({ settings: { enabled: false, interval: 'weekly' }, waiting: 'disabled' }) },
    }).body;
    expect(body).toContain('Automatic Clean');
    expect(body).toContain('AC power');
    expect(body).toContain('permanent');
    expect(body).toContain('aria-label="Clean automatically"');
    expect(body).not.toContain('automatic-clean-interval');
  });
});
