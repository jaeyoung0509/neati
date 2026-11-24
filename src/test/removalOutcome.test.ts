import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { RemovalResult } from '../lib/models/types';
import RemovalOutcomeCard from '../lib/components/RemovalOutcomeCard.svelte';
import { removalOutcome } from '../lib/utils/removalOutcome';

const kept = 'Skipped because a running process has files open in this folder.';

function result(removed: number, reasons: string[]): RemovalResult {
  return {
    removed_count: removed,
    failed_count: 0,
    skipped_count: reasons.length,
    removed_allocated_size: removed * 1024,
    items: [
      ...Array.from({ length: removed }, (_, index) => ({ item_id: `ok-${index}`, success: true, message: 'Removed' })),
      ...reasons.map((message, index) => ({ item_id: `kept-${index}`, success: false, message })),
    ],
  };
}

describe('reviewed removal outcome (#528)', () => {
  it('leads with why when nothing was removed', () => {
    const rendered = render(RemovalOutcomeCard, {
      props: { result: result(0, [kept]), singular: 'artifact', plural: 'artifacts' },
    });
    expect(rendered.body).toContain('Nothing was removed · 1 kept');
    expect(rendered.body).toContain('Why nothing was removed:');
    expect(rendered.body).toContain(kept);
    expect(rendered.body).not.toContain('<details');
    expect(rendered.body).not.toContain('Permanently removed 0');
  });

  it('folds the reasons away when something was removed', () => {
    const rendered = render(RemovalOutcomeCard, {
      props: { result: result(2, [kept]), singular: 'artifact', plural: 'artifacts' },
    });
    expect(rendered.body).toContain('Permanently removed 2 artifacts · 1 kept');
    expect(rendered.body).toContain('<details');
    expect(rendered.body).toContain('Why one was kept');
  });

  it('counts a repeated reason once', () => {
    const outcome = removalOutcome(result(0, [kept, kept, 'Skipped because the workspace changed after review.']), 'item', 'items');
    expect(outcome.reasons).toEqual([
      { message: kept, count: 2 },
      { message: 'Skipped because the workspace changed after review.', count: 1 },
    ]);
  });

  it('reports a clean run without a kept clause', () => {
    expect(removalOutcome(result(1, []), 'item', 'items').headline).toBe('Permanently removed 1 item');
  });
});
