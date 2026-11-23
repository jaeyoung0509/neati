import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import RemovalPlanCard from '../lib/components/RemovalPlanCard.svelte';
import { countLabel, planSizeLabel } from '../lib/utils/removalPlan';
import type { RemovalPlanPreview } from '../lib/models/types';
import { buttonTag, isDisabled } from './fixtures/markup';

const T0 = 1_800_000_000_000;

function plan(overrides: Partial<RemovalPlanPreview> = {}): RemovalPlanPreview {
  return {
    id: 'plan-1',
    item_count: 2,
    logical_size: 1_000_000,
    allocated_size: 2_097_152,
    expires_at: Math.floor(T0 / 1000) + 300,
    size_is_lower_bound: false,
    ...overrides,
  };
}

function props(overrides: Record<string, unknown> = {}) {
  return {
    plan: plan(),
    title: 'Permanent deletion review ready',
    summary: 'These files cannot be restored from Trash. 2 selected items · 2 MB allocated',
    confirmLabel: 'Permanently delete files',
    note: 'neati revalidates file identity and scope before each permanent deletion.',
    showExpiryTime: true,
    expiredMessage: 'Plan expired. Inventory is valid for 15 min — scan again to refresh.',
    expiryActionId: 'large-files-expiry-action',
    expiryActionLabel: 'Scan again',
    onExpiryAction: () => undefined,
    onDismiss: () => undefined,
    executing: false,
    onConfirm: () => undefined,
    onCancel: () => undefined,
    ...overrides,
  };
}

// One review card for Projects, Large Files and Apps (#491).
describe('RemovalPlanCard', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
  });
  afterEach(() => vi.useRealTimers());

  it('shows the title, count, size, countdown and permanent-removal copy', () => {
    const body = render(RemovalPlanCard, { props: props() }).body;
    expect(body).toContain('Permanent deletion review ready');
    expect(body).toContain('2 selected items · 2 MB allocated');
    expect(body).toContain('cannot be restored from Trash');
    expect(body).toContain('5:00 remaining');
    expect(body).toMatch(/One-shot, expires at .+ \(5:00 remaining\)\. neati revalidates file identity/);
    expect(isDisabled(buttonTag(body, 'Permanently delete files'))).toBe(false);
    expect(isDisabled(buttonTag(body, 'Cancel'))).toBe(false);
  });

  it('locks Cancel and the confirm while the deletion runs and says so', () => {
    const body = render(RemovalPlanCard, { props: props({ executing: true }) }).body;
    expect(isDisabled(buttonTag(body, 'Cancel'))).toBe(true);
    expect(isDisabled(buttonTag(body, 'Deleting…'))).toBe(true);
    expect(buttonTag(body, 'Deleting…')).toContain('aria-busy="true"');
    expect(body).toContain('Permanently deleting 2 items…');
  });

  it('keeps the confirm disabled while a view-owned consent is missing', () => {
    const body = render(RemovalPlanCard, { props: props({ confirmBlocked: true }) }).body;
    expect(isDisabled(buttonTag(body, 'Permanently delete files'))).toBe(true);
    expect(isDisabled(buttonTag(body, 'Cancel'))).toBe(false);
  });

  it('offers recovery with the view-owned id once the plan expires', () => {
    const body = render(RemovalPlanCard, {
      props: props({ plan: plan({ expires_at: Math.floor(T0 / 1000) - 1 }), expiredTitle: 'Plan expired — prepare again' }),
    }).body;
    expect(body).toContain('role="alert"');
    expect(body).toContain('Plan expired. Inventory is valid for 15 min');
    expect(body).toContain('id="large-files-expiry-action"');
    expect(body).toContain('Dismiss');
    expect(isDisabled(buttonTag(body, 'Expired'))).toBe(true);
    expect(buttonTag(body, 'Expired')).toContain('title="Plan expired — prepare again"');
    expect(body).not.toContain('One-shot, expires at');
  });

  it('qualifies a lower-bound size and pluralises the count', () => {
    expect(planSizeLabel(plan({ size_is_lower_bound: true }))).toBe('≥ 2 MB');
    expect(planSizeLabel(plan())).toBe('2 MB');
    expect(countLabel(1, 'selected artifact')).toBe('1 selected artifact');
    expect(countLabel(3, 'selected artifact')).toBe('3 selected artifacts');
  });

  it('is the only plan card in the three removal views', () => {
    for (const view of ['DeveloperArtifactsView', 'LargeFilesView', 'ApplicationsView']) {
      const source = readFileSync(new URL(`../routes/dashboard/${view}.svelte`, import.meta.url), 'utf8');
      expect(source, view).toContain('<RemovalPlanCard');
      expect(source, view).not.toContain('formatCountdown(');
      expect(source, view).not.toContain('ttlRemaining(');
    }
  });
});
