import type { AgentSkillState, AgentSkillTarget, CommandLineStatus } from '../models/types';

export const AGENT_SKILL_LABELS: Record<AgentSkillTarget, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
};

/** How the skill's state reads, and whether Install can act on it. */
export function agentSkillPresentation(state: AgentSkillState): {
  status: string;
  action: string | null;
  tone: 'success' | 'warning' | 'secondary';
} {
  switch (state) {
    case 'current':
      return { status: 'Installed', action: null, tone: 'success' };
    case 'outdated':
      return { status: 'Older version', action: 'Update', tone: 'warning' };
    case 'missing':
      return { status: 'Not installed', action: 'Install', tone: 'secondary' };
    case 'foreign':
      // neati never replaces a file it did not write.
      return { status: 'Another skill file is there', action: null, tone: 'warning' };
  }
}

/** One line on whether the `neati` command reaches this app. */
export function commandLineSummary(status: CommandLineStatus): string {
  if (status.linked_here) return `Available as neati (${status.command_path})`;
  if (status.command_path) return `neati on PATH is another copy (${status.command_path})`;
  return 'Not on PATH yet';
}
