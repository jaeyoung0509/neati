import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import WorktreeReview from '../lib/components/WorktreeReview.svelte';
import { WorktreeStore } from '../lib/stores/worktrees.svelte';
import { createWorktreesMock } from '../lib/api/mocks/worktrees';
import { worktreeStateDetail, worktreeStateLabel } from '../lib/utils/worktrees';
import type { WorktreeState } from '../lib/models/types';

const STATES: WorktreeState[] = [
  'finished',
  'recently_active',
  'activity_unknown',
  'has_changes',
  'has_submodules',
  'locked',
  'commits_only_here',
  'in_use',
  'use_unknown',
  'missing',
  'refused',
];

function store(refusal: string | null = null) {
  const mock = createWorktreesMock(() => refusal);
  return new WorktreeStore({
    scanWorktrees: () => mock.scan(),
    prepareWorktreeRemoval: (scanId, ids) => mock.prepare(scanId, ids),
    executeWorktreeRemoval: (planId) => mock.execute(planId),
    forgetMissingWorktrees: (scanId) => mock.forgetMissing(scanId),
  });
}

describe('Finished Git worktrees (#505)', () => {
  it('names every state and explains it', () => {
    for (const state of STATES) {
      expect(worktreeStateLabel(state).length).toBeGreaterThan(0);
    }
    const item = {
      id: 'w', repository: '/r', path: '/r-w', name: 'w', branch: null, head: 'abc1234', agent: null,
      allocated_bytes: 0, file_count: 0, measurement_complete: true, last_activity: null,
      state: 'refused' as const, reason: 'It is outside the home folder or in a protected location.',
      selected_by_default: false,
    };
    expect(worktreeStateDetail(item, 14)).toContain('protected');
    expect(worktreeStateDetail({ ...item, state: 'finished' }, 14)).toContain('14 days');
  });

  it('selects only finished worktrees and removes exactly the confirmed plan', async () => {
    const worktrees = store();
    await worktrees.scan();
    expect(worktrees.phase).toBe('ready');
    const blocked = worktrees.items.find((item) => item.state === 'has_changes')!;
    worktrees.toggle(blocked.id);
    expect(worktrees.selectedIds).toEqual([]);

    const [first] = worktrees.finished;
    worktrees.toggle(first.id);
    expect(worktrees.selectedBytes).toBe(first.allocated_bytes);
    await worktrees.review();
    expect(worktrees.phase).toBe('confirming');
    expect(worktrees.preview?.items.map((item) => item.id)).toEqual([first.id]);

    const result = await worktrees.remove();
    expect(result?.removed).toEqual([first.id]);
    expect(worktrees.preview).toBeNull();
    expect(worktrees.selectedIds).toEqual([]);
  });

  it('surfaces a refusal and keeps nothing pending', async () => {
    const worktrees = store('The browser preview does not remove worktrees. Use the neati app.');
    await worktrees.scan();
    worktrees.toggle(worktrees.finished[0].id);
    await worktrees.review();
    expect(await worktrees.remove()).toBeNull();
    expect(worktrees.error).toContain('browser preview');
    expect(worktrees.phase).toBe('ready');
  });

  it('renders states, agents and the rule hint', async () => {
    const worktrees = store();
    await worktrees.scan();
    const body = render(WorktreeReview, { props: { store: worktrees } }).body;
    expect(body).toContain('Git worktrees');
    expect(body).toContain('Uncommitted changes');
    expect(body).toContain('Codex');
    expect(body).toContain('turn on the worktree rule in Settings');
    expect(body).toContain('aria-label="Select worktree clean1"');
  });

  it('forgets gone worktrees through one explicit action (#523)', async () => {
    const worktrees = store();
    await worktrees.scan();
    expect(worktrees.missing).toHaveLength(1);
    const body = render(WorktreeReview, { props: { store: worktrees } }).body;
    expect(body).toContain('Forget 1 gone');
    expect(body).toContain('nothing on disk changes');
    await worktrees.forgetMissing();
    expect(worktrees.forgotten).toBe(1);
    expect(worktrees.error).toBeNull();
  });
});
