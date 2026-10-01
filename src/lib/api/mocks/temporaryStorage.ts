import type { TemporaryReviewConsent, TemporaryReviewPreview, TemporaryStorageEvent, TemporaryStorageInventory, TrashResult } from '../../models/types';

const observation = (state: 'in_use' | 'no_use_detected' | 'unable_to_determine', evidence: string[]) => ({
  state, observed_at: 1790812800, probe: 'Browser fixture · native use checks are unavailable', evidence,
  limitation: 'No use detected is not proof of abandonment. Future sessions, unpublished work and recovery were not verified.',
});

export const temporaryStorageFixture: TemporaryStorageInventory = {
  scan_id: 'temporary-preview', roots: ['/private/tmp', '/private/var/folders/fixture/T'],
  observed_at: 1790812800, expires_at: 1790813700, observed_allocated_bytes: 44_750_000_000,
  unknown_estimates: 1, partial: true, physical_overlap: false, cancelled: false, available: true, unavailable_reason: null,
  notes: ['Browser fixture, not a live inventory. Temporary review stays outside automatic cache totals.'],
  items: [
    { id: 'detached-target', name: 'neati-target-381', path: '/private/tmp/neati-target-381', logical_bytes: 27_000_000_000,
      allocated_bytes: 27_000_000_000, newest_activity: 1790809200, partial: false, physical_overlap: false, contents: ['build_output'],
      usage: observation('unable_to_determine', ['The fixture represents a timed-out use probe. Unknown use may be explicitly accepted for this whole folder.']),
      selected_by_default: false, options: [{ id: 'detached-whole', mode: 'whole_folder', path: '/private/tmp/neati-target-381', allocated_bytes: 27_000_000_000, partial: false, blocked_reason: null }] },
    { id: 'worktree', name: 'neati-issue-394', path: '/private/tmp/neati-issue-394', logical_bytes: 17_500_000_000,
      allocated_bytes: 17_500_000_000, newest_activity: 1790809800, partial: false, physical_overlap: false, contents: ['build_output', 'source_checkout', 'git_metadata'],
      usage: observation('no_use_detected', ['The completed fixture probe detected no open file, working directory or held build lock.']),
      selected_by_default: false, options: [
        { id: 'worktree-whole', mode: 'whole_folder', path: '/private/tmp/neati-issue-394', allocated_bytes: 17_500_000_000, partial: false, blocked_reason: null },
        { id: 'worktree-target', mode: 'generated_subtree', path: '/private/tmp/neati-issue-394/target', allocated_bytes: 17_000_000_000, partial: false, blocked_reason: null },
      ] },
    { id: 'browser', name: 'agent-browser-session', path: '/private/tmp/agent-browser-session', logical_bytes: 250_000_000,
      allocated_bytes: 250_000_000, newest_activity: 1790812800, partial: false, physical_overlap: false, contents: ['browser_or_session_data'],
      usage: observation('in_use', ['Open session data · browser (fixture PID 42)']), selected_by_default: false,
      options: [{ id: 'browser-whole', mode: 'whole_folder', path: '/private/tmp/agent-browser-session', allocated_bytes: 250_000_000, partial: false, blocked_reason: 'Active use detected. Stop the owner and scan again.' }] },
    { id: 'unclassified', name: 'unclassified-scratch', path: '/private/var/folders/fixture/T/unclassified-scratch', logical_bytes: null,
      allocated_bytes: null, newest_activity: null, partial: true, physical_overlap: false, contents: ['unknown'],
      usage: observation('unable_to_determine', ['An access failure left the structural inspection incomplete.']), selected_by_default: false,
      options: [{ id: 'unclassified-whole', mode: 'whole_folder', path: '/private/var/folders/fixture/T/unclassified-scratch', allocated_bytes: null, partial: true, blocked_reason: 'The unit cannot be inspected. Access and structural safety remain required.' }] },
  ],
};

export function createTemporaryStorageMock(fixture: TemporaryStorageInventory = temporaryStorageFixture) {
  let sequence = 0;
  let inventory: TemporaryStorageInventory | null = null;
  const drafts = new Map<string, TemporaryReviewPreview>();
  const cancelled = new Set<string>();
  const active = new Set<string>();
  return {
    async scan(onEvent: (event: TemporaryStorageEvent) => void): Promise<TemporaryStorageInventory> {
      const now = Math.floor(Date.now() / 1000);
      const next = structuredClone(fixture);
      next.scan_id = `temporary-preview-${++sequence}`;
      next.observed_at = now; next.expires_at = now + 900;
      next.items.forEach(item => { item.usage.observed_at = now; });
      active.add(next.scan_id);
      onEvent({ type: 'started', scan_id: next.scan_id });
      const measured: typeof next.items = [];
      for (const item of next.items) {
        if (cancelled.has(next.scan_id)) { next.cancelled = true; next.partial = true; break; }
        measured.push(item); onEvent({ type: 'item_found', item });
      }
      next.items = measured;
      next.observed_allocated_bytes = measured.reduce((total, item) => total + (item.allocated_bytes ?? 0), 0);
      next.unknown_estimates = measured.filter(item => item.partial || item.allocated_bytes === null).length;
      inventory = next;
      active.delete(next.scan_id);
      onEvent({ type: 'finished', inventory: next });
      return next;
    },
    async cancelScan(scanId: string): Promise<void> {
      if (!active.has(scanId)) throw new Error('The temporary scan is no longer running.');
      cancelled.add(scanId);
    },
    async prepare(scanId: string, selectedIds: string[]): Promise<TemporaryReviewPreview> {
      if (!inventory || inventory.scan_id !== scanId || inventory.expires_at <= Date.now() / 1000 || cancelled.has(scanId)) throw new Error('The temporary inventory expired or changed. Scan again.');
      if (!selectedIds.length) throw new Error('Select temporary scopes first.');
      const options = inventory.items.flatMap(item => item.options);
      const selected = [...new Set(selectedIds)].map(id => {
        const option = options.find(value => value.id === id);
        if (!option) throw new Error('The temporary selection changed.');
        if (option.blocked_reason) throw new Error(option.blocked_reason);
        return option;
      });
      const unknown = selected.some(option => inventory!.items.some(item => item.options.some(value => value.id === option.id) && item.usage.state === 'unable_to_determine'));
      if (selected.some(option => option.mode === 'generated_subtree' && inventory!.items.some(item => item.options.some(value => value.id === option.id) && item.usage.state !== 'no_use_detected'))) throw new Error('Generated-only cleanup requires complete use observations.');
      if (selected.some((option, index) => selected.some((other, otherIndex) => index !== otherIndex && (option.path.startsWith(other.path + '/') || other.path.startsWith(option.path + '/'))))) throw new Error('Choose one scope per folder.');
      const preview: TemporaryReviewPreview = {
        id: `temporary-plan-${++sequence}`, selected: structuredClone(selected),
        known_allocated_bytes: selected.reduce((sum, option) => sum + (option.allocated_bytes ?? 0), 0),
        unknown_estimates: selected.filter(option => option.allocated_bytes === null || option.partial).length,
        has_unknown_usage: unknown, has_whole_folders: selected.some(option => option.mode === 'whole_folder'),
        expires_at: Math.floor(Date.now() / 1000) + 300,
        warnings: ['No use detected does not establish abandonment or recovery.', 'Moves to Trash do not free disk space immediately. Restore manually to the original reviewed paths; Git worktree registrations may need repair.'],
      };
      drafts.set(preview.id, preview); return structuredClone(preview);
    },
    async execute(id: string, consent: TemporaryReviewConsent): Promise<TrashResult> {
      const preview = drafts.get(id); drafts.delete(id);
      if (!preview || preview.expires_at <= Date.now() / 1000) throw new Error('The review expired or was already used.');
      if (!consent.confirmed || (preview.has_unknown_usage && !consent.accept_unknown_usage) || (preview.has_whole_folders && !consent.accept_source_loss)) throw new Error('Confirm the exact scopes and their listed risks first.');
      return { moved_count: preview.selected.length, failed_count: 0, skipped_count: 0,
        moved_allocated_size: preview.known_allocated_bytes, size_is_lower_bound: !!preview.unknown_estimates,
        items: preview.selected.map(option => ({ item_id: option.id, success: true, message: `Preview only: ${option.path} would move to Trash. No files were changed.` })) };
    },
    async cancelExecution(id: string): Promise<void> { drafts.delete(id); },
  };
}
export const temporaryStorageMock = createTemporaryStorageMock();
