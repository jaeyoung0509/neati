import type {
  WorktreeInventory,
  WorktreeItem,
  WorktreeRemovalPreview,
  WorktreeRemovalResult,
} from '../../models/types';

const MIB = 1024 * 1024;
const DAY = 86_400;

/** Response fixtures in the backend's shape; never a second implementation. */
function fixtureItems(now: number): WorktreeItem[] {
  return [
    {
      id: 'worktree:0c1f2a3b4c5d',
      repository: '/Users/mock/Myproject/clean1',
      path: '/Users/mock/.codex/worktrees/422-readme-demo/clean1',
      name: 'clean1',
      branch: null,
      head: '5c19088',
      agent: 'codex',
      allocated_bytes: 49 * MIB,
      file_count: 1_204,
      measurement_complete: true,
      last_activity: now - 41 * DAY,
      state: 'finished',
      reason: null,
      selected_by_default: false,
    },
    {
      id: 'worktree:1d2e3f4a5b6c',
      repository: '/Users/mock/Myproject/clean1',
      path: '/Users/mock/Myproject/clean1-172-175',
      name: 'clean1-172-175',
      branch: 'feature/172-175-security-privacy-hardening',
      head: 'c5261f9',
      agent: null,
      allocated_bytes: 4 * MIB,
      file_count: 312,
      measurement_complete: true,
      last_activity: now - 73 * DAY,
      state: 'finished',
      reason: null,
      selected_by_default: false,
    },
    {
      id: 'worktree:2e3f4a5b6c7d',
      repository: '/Users/mock/Myproject/clean1',
      path: '/Users/mock/Myproject/clean1-worktrees/issue-416',
      name: 'issue-416',
      branch: 'feature/416-permanent-cleanup',
      head: '4a5d350',
      agent: null,
      allocated_bytes: 49 * MIB,
      file_count: 1_190,
      measurement_complete: true,
      last_activity: now - 44 * DAY,
      state: 'has_changes',
      reason: null,
      selected_by_default: false,
    },
    {
      id: 'worktree:3f4a5b6c7d8e',
      repository: '/Users/mock/Myproject/clean1',
      path: '/Users/mock/Myproject/clean1/.claude/worktrees/task-sessions',
      name: 'task-sessions',
      branch: 'feature/508-509-agent-sessions',
      head: 'c33cac5',
      agent: 'claude_code',
      allocated_bytes: 221 * MIB,
      file_count: 2_950,
      measurement_complete: true,
      last_activity: now - 1 * DAY,
      state: 'locked',
      reason: null,
      selected_by_default: false,
    },
  ];
}

export function createWorktreesMock(refusal: () => string | null) {
  let inventory: WorktreeInventory | null = null;
  const plans = new Map<string, string[]>();
  return {
    async scan(): Promise<WorktreeInventory> {
      const now = Math.floor(Date.now() / 1000);
      inventory = {
        scan_id: `mock-worktrees-${Date.now()}`,
        repositories: 3,
        items: fixtureItems(now),
        refused: [],
        idle_days: 14,
        rule_enabled: false,
      };
      return inventory;
    },
    async prepare(scanId: string, ids: string[]): Promise<WorktreeRemovalPreview> {
      if (!inventory || inventory.scan_id !== scanId) {
        throw new Error('These worktrees were found by an older scan. Scan again.');
      }
      const items = inventory.items.filter((item) => ids.includes(item.id));
      if (items.length === 0) throw new Error('Select at least one finished worktree.');
      const blocked = items.find((item) => item.state !== 'finished');
      if (blocked) throw new Error(`${blocked.name} is not finished, so it cannot be removed.`);
      const planId = crypto.randomUUID();
      plans.set(planId, ids);
      return {
        plan_id: planId,
        items,
        total_bytes: items.reduce((sum, item) => sum + item.allocated_bytes, 0),
        expires_in_seconds: 300,
      };
    },
    async execute(planId: string): Promise<WorktreeRemovalResult> {
      const refused = refusal();
      if (refused) throw new Error(refused);
      const ids = plans.get(planId);
      if (!ids) throw new Error('Worktree selection is no longer available. Review it again.');
      plans.delete(planId);
      return { removed: ids, failed: [], reclaimed_bytes: 0 };
    },
  };
}
