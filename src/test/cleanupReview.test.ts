import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import CleanupReviewDialog from '../lib/components/CleanupReviewDialog.svelte';
import { summarizeCleanupReview } from '../lib/utils/cleanupReview';
import type { PlanPreview, ScanItem } from '../lib/models/types';

const plan: PlanPreview = {
  id: 'fixture', expires_at: 1000, requires_confirmation: true, mode: 'mixed',
  expected_reclaim_bytes: 3072, refused: [],
  risk: { safe_count: 1, rebuild_count: 2, manual_count: 0, safe_bytes: 1024, rebuild_bytes: 2048, manual_bytes: 0 },
  targets: [
    { item_id: 'a', name: 'Cache one', path: '/private/one', mode: 'trash', expected_bytes: 2048, risk: 'rebuild', requires_confirmation: false },
    { item_id: 'b', name: 'Cache two', path: '/private/two', mode: 'permanent_delete', expected_bytes: 1024, risk: 'rebuild', requires_confirmation: true },
    { item_id: 'c', name: 'Prune', path: '/private/three', mode: 'permanent_delete', expected_bytes: 0, risk: 'rebuild', requires_confirmation: true },
  ],
};
const items = plan.targets.map(target => ({ id: target.item_id, cache_metadata: {
  provider: 'Example tool', consequence: 'Packages may download again.',
} })) as ScanItem[];

describe('cleanup review presentation', () => {
  it('uses backend modes, groups same-owner operations and retains unknown estimates', () => {
    const result = summarizeCleanupReview(plan, items);
    expect(result.deletedBytes).toBe(1024);
    expect(result.trashBytes).toBe(2048);
    expect(result.unestimated).toBe(1);
    expect(result.groups).toHaveLength(2);
    expect(result.groups.find(group => group.mode === 'permanent_delete')?.targets).toHaveLength(2);
  });
  it('keeps consequences separate even for the same owner', () => {
    const changed = items.map(item => ({ ...item, cache_metadata: { ...item.cache_metadata!, consequence: item.id } }));
    expect(summarizeCleanupReview(plan, changed).groups).toHaveLength(3);
  });
  it('does not infer owners from private paths and leaves plan unchanged', () => {
    const original = JSON.stringify(plan);
    expect(summarizeCleanupReview(plan).groups.every(group => group.label === 'Other selected items')).toBe(true);
    expect(JSON.stringify(plan)).toBe(original);
  });
  it('puts exact names and paths in closed disclosures, with clear impact and size copy', () => {
    const { body } = render(CleanupReviewDialog, { props: { plan, items, onCancel() {}, onConfirm() {} } });
    expect(body).toContain('Example tool');
    expect(body).toContain('Packages may download again.');
    expect(body).toContain('still use disk space');
    expect(body).toContain('1 action has no size estimate');
    expect(body).not.toContain('Varies');
    expect(body).not.toMatch(/<details[^>]*\bopen/);
    expect(body).toMatch(/<details[\s\S]*\/private\/one[\s\S]*<\/details>/);
    expect(body).toContain('<footer');
  });
});
