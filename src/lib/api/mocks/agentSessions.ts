import type { AgentSessionInventory, AgentSessionProject, AgentSessionSummary } from '../../models/types';

/** A fixed clock so the preview renders the same dates every time. */
const NOW = 1_795_515_072;
const HOUR = 3_600;

function session(
  key: string,
  overrides: Partial<AgentSessionSummary> & Pick<AgentSessionSummary, 'tool' | 'session_id'>
): AgentSessionSummary {
  return {
    key,
    cwd: null,
    title: null,
    started_at: NOW - 3 * HOUR,
    last_activity_at: NOW - HOUR,
    dates_from_records: true,
    size_bytes: 1_048_576,
    file_count: 1,
    archived: false,
    issue: null,
    ...overrides,
  };
}

function project(key: string, cwd: string, folder: AgentSessionProject['folder'], sessions: AgentSessionSummary[]): AgentSessionProject {
  return {
    key,
    cwd,
    display_name: cwd.split('/').filter(Boolean).pop() ?? cwd,
    folder,
    total_bytes: sessions.reduce((sum, entry) => sum + entry.size_bytes, 0),
    last_activity_at: Math.max(...sessions.map((entry) => entry.last_activity_at ?? 0)),
    sessions,
  };
}


/** Browser-preview fixture: two projects, one of them gone from this Mac. */
export function createAgentSessionsMock() {
  const app = '/Users/mock/code/neati';
  const gone = '/Users/mock/code/old-prototype';
  const projects: AgentSessionProject[] = [
    project('p-neati', app, 'present', [
      session('c0ffee0000000001', {
        tool: 'claude',
        session_id: '0b5c7e2a-1111-4c3e-9a1b-2c3d4e5f6a7b',
        cwd: app,
        title: 'Add a Sessions tab to the AI section',
        size_bytes: 48_234_496,
        file_count: 87,
      }),
      session('c0ffee0000000002', {
        tool: 'codex',
        session_id: '019a0000-2222-7abc-8def-0123456789ab',
        cwd: app,
        title: 'Wire the release workflow to the signing key',
        last_activity_at: NOW - 5 * HOUR,
        size_bytes: 4_493_189,
      }),
    ]),
    project('p-gone', gone, 'missing', [
      session('c0ffee0000000003', {
        tool: 'codex',
        session_id: '019a0000-3333-7abc-8def-0123456789ab',
        cwd: gone,
        title: null,
        last_activity_at: NOW - 30 * 24 * HOUR,
        dates_from_records: false,
        size_bytes: 20_688,
        archived: true,
      }),
    ]),
  ];
  const all = projects.flatMap((entry) => entry.sessions);

  return {
    async inventory(): Promise<AgentSessionInventory> {
      return {
        observed_at: NOW,
        projects: structuredClone(projects),
        tools: (['claude', 'codex'] as const).map((tool) => {
          const sessions = all.filter((entry) => entry.tool === tool);
          return {
            tool,
            present: true,
            sessions: sessions.length,
            files: sessions.reduce((sum, entry) => sum + entry.file_count, 0),
            bytes: sessions.reduce((sum, entry) => sum + entry.size_bytes, 0),
          };
        }),
        truncated: false,
        cancelled: false,
        incomplete_reasons: [],
      };
    },
  };
}
