import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import CleanupHistory from '../lib/components/CleanupHistory.svelte';
import { cleanupHistoryIssues, cleanupHistoryTotals } from '../lib/utils/cleanupHistory';
import type { CleanupHistoryEntry } from '../lib/models/types';

const entry = (overrides: Partial<CleanupHistoryEntry> = {}): CleanupHistoryEntry => ({
  finished_at: 1795300000,
  item_count: 4,
  removed_bytes: 1000,
  partial_count: 0,
  failed_count: 0,
  skipped_count: 0,
  ...overrides,
});

describe('cleanup history', () => {
  it('totals removed bytes across runs', () => {
    expect(cleanupHistoryTotals([entry(), entry({ removed_bytes: 500 })])).toEqual({ runs: 2, removedBytes: 1500 });
    expect(cleanupHistoryTotals([])).toEqual({ runs: 0, removedBytes: 0 });
  });

  it('names only the outcomes that happened', () => {
    expect(cleanupHistoryIssues(entry())).toBe('');
    expect(cleanupHistoryIssues(entry({ partial_count: 1, skipped_count: 2 }))).toBe('1 partly cleaned · 2 skipped');
    expect(cleanupHistoryIssues(entry({ failed_count: 3 }))).toBe('3 failed');
  });

  it('renders a loading state before any history is read', () => {
    const { body } = render(CleanupHistory);
    expect(body).toContain('Reading cleanup history');
    expect(body).toContain('role="status"');
  });
});
