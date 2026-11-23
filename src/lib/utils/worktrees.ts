import type { WorktreeAgent, WorktreeItem, WorktreeState } from '../models/types';

/** A few words for a worktree's state. */
export function worktreeStateLabel(state: WorktreeState): string {
  switch (state) {
    case 'finished':
      return 'Finished';
    case 'recently_active':
      return 'Recently worked on';
    case 'activity_unknown':
      return 'Activity unknown';
    case 'has_changes':
      return 'Uncommitted changes';
    case 'has_submodules':
      return 'Has submodules';
    case 'locked':
      return 'Locked';
    case 'commits_only_here':
      return 'Commits only here';
    case 'in_use':
      return 'In use';
    case 'use_unknown':
      return 'Use unknown';
    case 'missing':
      return 'Folder gone';
    case 'refused':
      return 'Not checked';
    default:
      return 'Not checked';
  }
}

/** Why a worktree is or is not removable, for a tooltip or detail line. */
export function worktreeStateDetail(item: WorktreeItem, idleDays: number): string {
  switch (item.state) {
    case 'finished':
      return `Everything is committed and on a branch, nothing uses it, and nobody has worked in it for ${idleDays} days.`;
    case 'recently_active':
      return `Worked on within ${idleDays} days, or a process works inside it.`;
    case 'activity_unknown':
      return 'When it was last worked on could not be observed.';
    case 'has_changes':
      return 'It has uncommitted or untracked files that exist only here.';
    case 'has_submodules':
      return 'Git does not remove a worktree with submodules without force; neati never forces.';
    case 'locked':
      return 'It is locked with git worktree lock.';
    case 'commits_only_here':
      return 'Its detached commit is on no branch or tag, so removing it could lose that commit.';
    case 'in_use':
      return 'A running process has files open in it.';
    case 'use_unknown':
      return 'neati could not confirm that nothing is using it.';
    case 'missing':
      return 'Git still lists it, but the folder is gone. Nothing to remove here.';
    case 'refused':
      return item.reason ?? 'It could not be checked safely.';
    default:
      return 'It could not be checked safely.';
  }
}

export function worktreeAgentLabel(agent: WorktreeAgent | null): string | null {
  switch (agent) {
    case 'codex':
      return 'Codex';
    case 'claude_code':
      return 'Claude Code';
    default:
      return null;
  }
}
