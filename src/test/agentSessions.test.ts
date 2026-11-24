import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { readFileSync } from 'node:fs';
import AgentSessionsView from '../routes/dashboard/AgentSessionsView.svelte';
import { mockApi } from '../lib/api/mock';
import { nativeApi } from '../lib/api/native';
import { MOCK_SESSION_WITH_FINDING } from '../lib/api/mocks/agentSessions';
import { agentSessionsStore } from '../lib/stores/agentSessions.svelte';
import { SECTIONS, dashboardStage, sectionForRoute, sectionTabForRoute } from '../lib/utils/dashboardNavigation';
import {
  SECRET_KIND_LABELS,
  bundleSummary,
  findingLines,
  folderStateLabel,
  sessionIssueLabel,
  sessionLabel,
} from '../lib/utils/agentSessions';
import type { AgentSessionInventory, SecretKind } from '../lib/models/types';

const SESSION_COMMANDS = [
  'getAgentSessions',
  'prepareSessionExport',
  'exportSessionBundle',
  'openSessionBundle',
  'pickSessionProjectFolder',
  'importSessionBundle',
  'cancelSessionOperation',
] as const;

function reset() {
  agentSessionsStore.inventory = null;
  agentSessionsStore.phase = 'idle';
  agentSessionsStore.error = null;
  agentSessionsStore.selected = [];
  agentSessionsStore.exportStep = 'idle';
  agentSessionsStore.exportPreview = null;
  agentSessionsStore.importStep = 'idle';
  agentSessionsStore.bundle = null;
}

afterEach(reset);

describe('Sessions navigation (#508)', () => {
  it('is a Beta tab in the Agents section, not a sidebar entry', () => {
    const ai = SECTIONS.find((section) => section.id === 'ai')!;
    expect(ai.label).toBe('Agents');
    expect(ai.routes.filter((entry) => !entry.more).map((entry) => entry.label)).toEqual(['Activity', 'Sessions']);
    expect(SECTIONS.map((section) => section.id)).toEqual(['home', 'clean', 'ai', 'settings']);
    expect(sectionForRoute('sessions').id).toBe('ai');
    expect(sectionTabForRoute('sessions')).toBe('sessions');
    expect(dashboardStage('sessions')).toBe('beta');
  });

  it('keeps the view title equal to its tab label', () => {
    const source = readFileSync(new URL('../routes/dashboard/AgentSessionsView.svelte', import.meta.url), 'utf8');
    expect(source).toMatch(/title="Sessions"/);
  });
});

describe('Sessions preview API (#508, #509)', () => {
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

  it('requires confirmation for findings and holds each preview once', async () => {
    const preview = await mockApi.prepareSessionExport([MOCK_SESSION_WITH_FINDING]);
    expect(preview.findings).toEqual([{ kind: 'anthropic_key', count: 1 }]);
    await expect(mockApi.exportSessionBundle(preview.id, false)).rejects.toThrow('Likely secrets');
    const outcome = await mockApi.exportSessionBundle(preview.id, true);
    expect(outcome?.sessions).toBe(1);
    await expect(mockApi.exportSessionBundle(preview.id, true)).rejects.toThrow('expired');
    await expect(mockApi.prepareSessionExport([])).rejects.toThrow('at least one');
    await expect(mockApi.prepareSessionExport(['nope'])).rejects.toThrow('Nothing was exported');

    const bundle = await mockApi.openSessionBundle();
    expect(bundle?.projects.some((project) => project.folder === 'missing')).toBe(true);
    const folder = await mockApi.pickSessionProjectFolder();
    const imported = await mockApi.importSessionBundle(bundle!.id, [{ from: '/Users/teammate/code/api', to: folder! }]);
    expect(imported.kept_existing).toHaveLength(1);
    expect(imported.written_files).toBeGreaterThan(0);
    await expect(mockApi.importSessionBundle(bundle!.id, [])).rejects.toThrow('expired');
  });

  it('refuses to write a bundle while showing preview data', async () => {
    await agentSessionsStore.load();
    const key = agentSessionsStore.inventory!.projects[0].sessions[0].key;
    agentSessionsStore.setSelected([key], true);
    await agentSessionsStore.prepareExport();
    expect(agentSessionsStore.exportStep).toBe('review');
    await agentSessionsStore.exportBundle(false);
    expect(agentSessionsStore.exportError).toContain('preview data');
    expect(agentSessionsStore.exportResult).toBeNull();
  });
});

describe('Sessions copy', () => {
  it('names every secret kind and shows counts, never values', () => {
    const kinds: SecretKind[] = [
      'private_key',
      'anthropic_key',
      'openai_style_key',
      'github_token',
      'github_fine_grained_token',
      'aws_access_key',
      'slack_token',
    ];
    expect(Object.keys(SECRET_KIND_LABELS).sort()).toEqual([...kinds].sort());
    expect(findingLines([{ kind: 'github_token', count: 2 }])).toEqual(['2 × GitHub token (ghp_)']);
  });

  it('reads folder states, issues and sizes plainly', () => {
    expect(folderStateLabel('missing')).toBe('Folder gone');
    expect(folderStateLabel('present')).toBeNull();
    expect(sessionIssueLabel(null)).toBeNull();
    expect(sessionIssueLabel('not_read')).toContain('read limit');
    expect(bundleSummary(1, 3, 2048)).toBe('1 session · 3 files · 2 KB');
    expect(sessionLabel(null, '0b5c7e2a-1111')).toBe('Session 0b5c7e2a');
  });
});

describe('Sessions view', () => {
  it('waits for the first answer instead of showing an empty list', () => {
    const body = render(AgentSessionsView).body;
    expect(body).toContain('Reading agent sessions');
    expect(body).not.toContain('No sessions yet');
  });

  it('groups sessions by project, flags a missing folder and labels every checkbox', async () => {
    const inventory: AgentSessionInventory = await mockApi.getAgentSessions();
    agentSessionsStore.inventory = inventory;
    agentSessionsStore.phase = 'ready';
    const body = render(AgentSessionsView).body;
    expect(body).toMatch(/<h1[^>]*>Sessions<\/h1>/);
    expect(body).toContain('Folder gone');
    expect(body).toContain('old-prototype');
    expect(body).toContain('aria-label="Select every session in neati"');
    expect(body).toContain('aria-label="Select Add a Sessions tab to the AI section"');
    expect(body).toContain('Import bundle…');
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
