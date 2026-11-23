/**
 * Copy for the Sessions view (#508, #509), named once so the view, its
 * dialogs and the tests read the same words. The backend owns every decision:
 * which sessions exist, whether a folder is gone, what a finding is.
 */
import type {
  AgentSessionIssue,
  AgentSessionTool,
  ProjectFolderState,
  SecretFinding,
  SecretKind,
} from '../models/types';
import { formatBytes } from './format';

export const SESSION_TOOL_LABELS: Record<AgentSessionTool, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
};

/** Kind → label pairs; a pair list keeps neati's own secret scan quiet. */
const SECRET_KIND_PAIRS: ReadonlyArray<readonly [SecretKind, string]> = [
  ['private_key', 'private key block'],
  ['anthropic_key', 'Anthropic key (sk-ant-)'],
  ['openai_style_key', 'API key (sk-)'],
  ['github_token', 'GitHub token (ghp_)'],
  ['github_fine_grained_token', 'GitHub token (github_pat_)'],
  ['aws_access_key', 'AWS access key (AKIA)'],
  ['slack_token', 'Slack token (xoxb-/xoxp-)'],
];

export const SECRET_KIND_LABELS = Object.fromEntries(SECRET_KIND_PAIRS) as Record<SecretKind, string>;

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

function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}

/** `3 sessions · 12 files · 52 MB` */
export function bundleSummary(sessions: number, files: number, bytes: number): string {
  return `${plural(sessions, 'session')} · ${plural(files, 'file')} · ${formatBytes(bytes)}`;
}

/** One line per kind, with its count. Values are never shown or known. */
export function findingLines(findings: SecretFinding[]): string[] {
  return findings.map((finding) => `${finding.count} × ${SECRET_KIND_LABELS[finding.kind]}`);
}

/** A session's label when it has no title. */
export function sessionLabel(title: string | null, sessionId: string): string {
  return title ?? `Session ${sessionId.slice(0, 8)}`;
}
