import type { AiProfileTool, AiToolProfile } from '../models/types';

export const AI_PROFILE_TOOLS: ReadonlyArray<{ id: AiProfileTool; label: string; defaultDir: string }> = [
  { id: 'claude', label: 'Claude Code', defaultDir: '~/.claude' },
  { id: 'codex', label: 'Codex', defaultDir: '~/.codex' },
  { id: 'gemini', label: 'Gemini / Antigravity', defaultDir: '~/.gemini' },
  { id: 'opencode', label: 'OpenCode', defaultDir: '~/.opencode' },
  { id: 'cursor', label: 'Cursor', defaultDir: '~/.cursor' },
];

/** Must match the backend cap; the backend refuses a longer list regardless. */
export const MAX_AI_TOOL_PROFILES = 8;

export function aiProfileToolLabel(tool: AiProfileTool): string {
  return AI_PROFILE_TOOLS.find((entry) => entry.id === tool)?.label ?? tool;
}

export function hasProfile(profiles: readonly AiToolProfile[], candidate: AiToolProfile): boolean {
  return profiles.some((profile) => profile.tool === candidate.tool && profile.path === candidate.path);
}

/** `/Users/me/.claude-work` → `.claude-work`, for a compact row label. */
export function profileFolderName(path: string): string {
  const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
