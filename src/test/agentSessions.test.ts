import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { readFileSync } from 'node:fs';
import AgentSessionsView from '../routes/dashboard/AgentSessionsView.svelte';
import { mockApi } from '../lib/api/mock';
import { nativeApi } from '../lib/api/native';
import { agentSessionsStore } from '../lib/stores/agentSessions.svelte';
import { SECTIONS, dashboardStage, sectionForRoute, sectionTabForRoute } from '../lib/utils/dashboardNavigation';
import { folderStateLabel, sessionIssueLabel, sessionLabel } from '../lib/utils/agentSessions';
import type { AgentSessionInventory } from '../lib/models/types';

const SESSION_COMMANDS = ['getAgentSessions', 'cancelSessionOperation'] as const;

function reset() {
  agentSessionsStore.inventory = null;
  agentSessionsStore.phase = 'idle';
  agentSessionsStore.error = null;
}

afterEach(reset);

describe('Sessions navigation (#508)', () => {
  it('is a Beta tab in the AI section, not a sidebar entry', () => {
    const ai = SECTIONS.find((section) => section.id === 'ai')!;
    expect(ai.routes.map((entry) => entry.label)).toEqual(['Activity', 'Sessions', 'Controls']);
    expect(SECTIONS.map((section) => section.id)).toEqual(['home', 'clean', 'system', 'ai', 'settings']);
    expect(sectionForRoute('sessions').id).toBe('ai');
    expect(sectionTabForRoute('sessions')).toBe('sessions');
    expect(dashboardStage('sessions')).toBe('beta');
  });

  it('keeps the view title equal to its tab label', () => {
    const source = readFileSync(new URL('../routes/dashboard/AgentSessionsView.svelte', import.meta.url), 'utf8');
    expect(source).toMatch(/title="Sessions"/);
  });
});

describe('Sessions preview API (#508)', () => {
  it('serves every session command in both bridges', () => {
    for (const command of SESSION_COMMANDS) {
      expect(typeof nativeApi[command], command).toBe('function');
      expect(typeof mockApi[command], command).toBe('function');
    }
  });

  it('lists projects with an orphan and per-tool totals', async () => {
    const inventory = await mockApi.getAgentSessions();
    expect(inventory.projects.map((project) => project.folder)).toEqual(['present', 'missing']);
    expect(inventory.tools.map((tool) => tool.tool)).toEqual(['claude', 'codex']);
    const sessions = inventory.projects.flatMap((project) => project.sessions);
    expect(inventory.tools.reduce((sum, tool) => sum + tool.sessions, 0)).toBe(sessions.length);
    for (const project of inventory.projects) {
      expect(project.total_bytes).toBe(project.sessions.reduce((sum, entry) => sum + entry.size_bytes, 0));
    }
  });
});

describe('Sessions copy', () => {
  it('reads folder states, issues and labels plainly', () => {
    expect(folderStateLabel('missing')).toBe('Folder gone');
    expect(folderStateLabel('present')).toBeNull();
    expect(sessionIssueLabel(null)).toBeNull();
    expect(sessionIssueLabel('not_read')).toContain('read limit');
    expect(sessionLabel(null, '0b5c7e2a-1111')).toBe('Session 0b5c7e2a');
  });
});

describe('Sessions view', () => {
  it('waits for the first answer instead of showing an empty list', () => {
    const body = render(AgentSessionsView).body;
    expect(body).toContain('Reading agent sessions');
    expect(body).not.toContain('No sessions yet');
  });

  it('groups sessions by project and flags a missing folder', async () => {
    const inventory: AgentSessionInventory = await mockApi.getAgentSessions();
    agentSessionsStore.inventory = inventory;
    agentSessionsStore.phase = 'ready';
    const body = render(AgentSessionsView).body;
    expect(body).toMatch(/<h1[^>]*>Sessions<\/h1>/);
    expect(body).toContain('Folder gone');
    expect(body).toContain('old-prototype');
    expect(body).toContain('Add a Sessions tab to the AI section');
    expect(body).toContain('Read sessions again');
    expect(body).toContain('Archived');
  });

  it('names a home without session folders', () => {
    agentSessionsStore.inventory = {
      observed_at: 1,
      projects: [],
      tools: [
        { tool: 'claude', present: false, sessions: 0, files: 0, bytes: 0 },
        { tool: 'codex', present: false, sessions: 0, files: 0, bytes: 0 },
      ],
      truncated: false,
      cancelled: false,
      incomplete_reasons: [],
    };
    agentSessionsStore.phase = 'ready';
    expect(render(AgentSessionsView).body).toContain('No session folders found');
  });
});
