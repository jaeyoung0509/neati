import type {
  AgentSessionInventory,
  AgentSessionProject,
  AgentSessionSummary,
  SessionBundlePreview,
  SessionExportOutcome,
  SessionExportPreview,
  SessionImportOutcome,
  SessionRemap,
} from '../../models/types';

/** A fixed clock so the preview renders the same dates every time. */
const NOW = 1_795_515_072;
const HOUR = 3_600;
const EXPIRED = 'This review expired or was already used. Prepare it again.';

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

/** The session whose preview export reports a likely secret. */
export const MOCK_SESSION_WITH_FINDING = 'c0ffee0000000002';

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
      session(MOCK_SESSION_WITH_FINDING, {
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
  const exports = new Map<string, SessionExportPreview>();
  const imports = new Map<string, SessionBundlePreview>();
  let counter = 0;
  const nextId = () => `00000000-0000-4000-8000-${String(++counter).padStart(12, '0')}`;

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

    async prepareExport(keys: string[]): Promise<SessionExportPreview> {
      if (keys.length === 0) throw new Error('Choose at least one session to export.');
      const chosen = keys.map((key) => {
        const found = all.find((entry) => entry.key === key);
        if (!found) throw new Error(`${key} is not a session in the current inventory. Nothing was exported.`);
        return found;
      });
      const preview: SessionExportPreview = {
        id: nextId(),
        sessions: chosen.length,
        files: chosen.reduce((sum, entry) => sum + entry.file_count, 0),
        bytes: chosen.reduce((sum, entry) => sum + entry.size_bytes, 0),
        findings: keys.includes(MOCK_SESSION_WITH_FINDING) ? [{ kind: 'anthropic_key', count: 1 }] : [],
      };
      exports.set(preview.id, preview);
      return preview;
    },

    async exportBundle(previewId: string, allowSecrets: boolean): Promise<SessionExportOutcome | null> {
      const preview = exports.get(previewId);
      if (!preview) throw new Error(EXPIRED);
      if (preview.findings.length > 0 && !allowSecrets) {
        throw new Error('Likely secrets were found. Confirm that the bundle may include them.');
      }
      exports.delete(previewId);
      return {
        path: '/Users/mock/Desktop/agent-sessions.neati-sessions',
        sessions: preview.sessions,
        files: preview.files,
        bytes: preview.bytes,
        archive_bytes: Math.round(preview.bytes / 8),
        findings: preview.findings,
      };
    },

    async openBundle(): Promise<SessionBundlePreview | null> {
      const preview: SessionBundlePreview = {
        id: nextId(),
        name: 'agent-sessions.neati-sessions',
        neati_version: '0.3.135',
        sessions: [
          {
            tool: 'claude',
            session_id: '7d1e0000-4444-4c3e-9a1b-2c3d4e5f6a7b',
            original_cwd: '/Users/teammate/code/api',
            files: 3,
            bytes: 2_097_152,
            existing_files: 0,
          },
          {
            tool: 'codex',
            session_id: '019a0000-5555-7abc-8def-0123456789ab',
            original_cwd: app,
            files: 1,
            bytes: 524_288,
            existing_files: 1,
          },
        ],
        projects: [
          { original_cwd: '/Users/teammate/code/api', folder: 'missing', sessions: 1 },
          { original_cwd: app, folder: 'present', sessions: 1 },
        ],
      };
      imports.set(preview.id, preview);
      return preview;
    },

    async pickProjectFolder(): Promise<string | null> {
      return '/Users/mock/code/api';
    },

    async importBundle(previewId: string, remaps: SessionRemap[]): Promise<SessionImportOutcome> {
      const preview = imports.get(previewId);
      if (!preview) throw new Error(EXPIRED);
      imports.delete(previewId);
      const kept = preview.sessions.filter((entry) => entry.existing_files > 0);
      return {
        sessions: preview.sessions.filter((entry) => entry.existing_files < entry.files).length,
        written_files: preview.sessions.reduce((sum, entry) => sum + entry.files - entry.existing_files, 0),
        kept_existing: kept.map((entry) => `${entry.tool}/sessions/${entry.session_id}.jsonl`),
        rewritten_files: remaps.length > 0 ? 2 : 0,
      };
    },
  };
}
