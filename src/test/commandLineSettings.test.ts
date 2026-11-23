import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SettingsCommandLine from '../lib/components/settings/SettingsCommandLine.svelte';
import { agentSkillPresentation, commandLineSummary } from '../lib/utils/commandLine';
import { mockApi } from '../lib/api/mock';
import type { CommandLineStatus } from '../lib/models/types';

const status: CommandLineStatus = {
  executable: '/Applications/neati.app/Contents/MacOS/Neati',
  command_path: null,
  linked_here: false,
  link_command: 'ln -sf "/Applications/neati.app/Contents/MacOS/Neati" /opt/homebrew/bin/neati',
  skills: [
    { target: 'claude', path: '/Users/me/.claude/skills/neati/SKILL.md', state: 'outdated' },
    { target: 'codex', path: '/Users/me/.codex/skills/neati/SKILL.md', state: 'foreign' },
  ],
};

describe('Settings → Command line and agents (#500)', () => {
  it('offers the link command and only the installs neati may perform', () => {
    const body = render(SettingsCommandLine, { props: { initialStatus: status } }).body;
    expect(body).toContain('Not on PATH yet');
    expect(body).toContain('/opt/homebrew/bin/neati');
    expect(body).toContain('aria-label="Copy the link command"');
    expect(body).toContain('Older version');
    expect(body).toContain('>Update<');
    // A file neati did not write is never offered for replacement.
    expect(body).toContain('Another skill file is there');
    expect(body.match(/>Install</g)).toBeNull();
  });

  it('reads the command state and the skill states honestly', () => {
    expect(commandLineSummary({ ...status, linked_here: true, command_path: '/opt/homebrew/bin/neati' }))
      .toBe('Available as neati (/opt/homebrew/bin/neati)');
    expect(commandLineSummary({ ...status, command_path: '/usr/local/bin/neati' }))
      .toContain('another copy');
    expect(agentSkillPresentation('current').action).toBeNull();
    expect(agentSkillPresentation('missing').action).toBe('Install');
    expect(agentSkillPresentation('foreign').action).toBeNull();
  });

  it('keeps the browser preview to the same shapes and refuses foreign files', async () => {
    const preview = await mockApi.getCommandLineStatus();
    expect(preview.skills.map((skill) => skill.target)).toEqual(['claude', 'codex']);
    const installed = await mockApi.installAgentSkill('codex');
    expect(installed).toMatchObject({ target: 'codex', replaced: false });
  });
});
