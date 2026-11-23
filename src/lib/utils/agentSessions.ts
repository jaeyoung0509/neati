/**
 * Copy for the Sessions view (#508), named once so the view and the tests
 * read the same words. The backend owns every decision: which sessions exist
 * and whether a folder is gone.
 */
import type { AgentSessionIssue, AgentSessionTool, ProjectFolderState } from '../models/types';

export const SESSION_TOOL_LABELS: Record<AgentSessionTool, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
};

/** A short badge for a project folder, or `null` when there is nothing to say. */
export function folderStateLabel(state: ProjectFolderState): string | null {
  if (state === 'missing') return 'Folder gone';
  if (state === 'unknown') return 'Folder not checked';
  return null;
}

export function sessionIssueLabel(issue: AgentSessionIssue | null): string | null {
  switch (issue) {
    case 'unreadable':
      return 'Could not be read';
    case 'no_records':
      return 'No records neati recognises';
    case 'not_read':
      return 'Not read: the read limit was reached';
    default:
      return null;
  }
}

/** A session's label when it has no title. */
export function sessionLabel(title: string | null, sessionId: string): string {
  return title ?? `Session ${sessionId.slice(0, 8)}`;
}
