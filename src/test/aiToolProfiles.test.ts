import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import AiToolProfiles from '../lib/components/AiToolProfiles.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { AI_PROFILE_TOOLS, aiProfileToolLabel, hasProfile, profileFolderName } from '../lib/utils/aiToolProfiles';

describe('AI tool profiles', () => {
  it('covers every tool that keeps a profile folder, with its default location', () => {
    expect(AI_PROFILE_TOOLS.map((tool) => tool.id)).toEqual(['claude', 'codex', 'gemini', 'opencode', 'cursor']);
    for (const tool of AI_PROFILE_TOOLS) expect(tool.defaultDir).toMatch(/^~\/\.[a-z]+$/);
    expect(aiProfileToolLabel('codex')).toBe('Codex');
  });

  it('compares profiles by tool and path and names the folder', () => {
    const profiles = [{ tool: 'claude' as const, path: '/Users/me/.claude-work' }];
    expect(hasProfile(profiles, { tool: 'claude', path: '/Users/me/.claude-work' })).toBe(true);
    expect(hasProfile(profiles, { tool: 'codex', path: '/Users/me/.claude-work' })).toBe(false);
    expect(profileFolderName('/Users/me/.claude-work/')).toBe('.claude-work');
  });

  it('lists saved profiles with a labelled remove action and states they are never cleaned automatically', () => {
    const previous = settingsStore.settings;
    settingsStore.settings = { ...previous, ai_tool_profiles: [{ tool: 'claude', path: '/Users/me/.claude-work' }] };
    try {
      const { body } = render(AiToolProfiles);
      expect(body).toContain('Claude Code · .claude-work');
      expect(body).toContain('aria-label="Remove Claude Code profile .claude-work"');
      expect(body).toContain('never cleaned automatically');
      expect(body).toContain('Profile folder (full path)');
    } finally {
      settingsStore.settings = previous;
    }
  });
});
