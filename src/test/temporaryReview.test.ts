import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import TemporaryStorageView from '../routes/dashboard/TemporaryStorageView.svelte';
import { TemporaryReviewStore } from '../lib/stores/temporaryReview.svelte';
import { createTemporaryStorageMock, temporaryStorageFixture } from '../lib/api/mocks/temporaryStorage';
import nativeContract from '../lib/bindings/temporary-storage.golden.json';

const consent = { confirmed: true, accept_unknown_usage: true, accept_source_loss: true };
const keys = (value: object) => Object.keys(value).sort();
afterEach(() => vi.useRealTimers());

describe('dedicated temporary review', () => {
  it('matches native serialized response keys and keeps all usage states unselected', async () => {
    const mock = createTemporaryStorageMock();
    const events: string[] = [];
    const inventory = await mock.scan(event => events.push(event.type));
    expect(events[0]).toBe('started'); expect(events.at(-1)).toBe('finished');
    expect(keys(inventory)).toEqual(keys(nativeContract.inventory));
    for (const item of inventory.items) {
      expect(keys(item)).toEqual(keys(nativeContract.inventory.items[0]));
      expect(keys(item.usage)).toEqual(keys(nativeContract.inventory.items[0].usage));
      expect(item.selected_by_default).toBe(false);
      for (const option of item.options) expect(keys(option)).toEqual(keys(nativeContract.inventory.items[0].options[0]));
    }
    expect(new Set(inventory.items.map(item => item.usage.state))).toEqual(new Set(['in_use', 'no_use_detected', 'unable_to_determine']));
    const preview = await mock.prepare(inventory.scan_id, ['detached-whole']);
    expect(keys(preview)).toEqual(keys(nativeContract.preview));
    expect(preview).toMatchObject({ has_unknown_usage: true, has_whole_folders: true });
  });

  it('rejects active, forged, overlapping and generated-unknown scopes before consent', async () => {
    const mock = createTemporaryStorageMock(); const inventory = await mock.scan(() => {});
    await expect(mock.prepare(inventory.scan_id, ['browser-whole'])).rejects.toThrow('Active use');
    await expect(mock.prepare(inventory.scan_id, ['detached-whole', 'forged'])).rejects.toThrow('selection changed');
    await expect(mock.prepare(inventory.scan_id, ['worktree-whole', 'worktree-target'])).rejects.toThrow('one scope');
    const unknownGenerated = structuredClone(temporaryStorageFixture);
    unknownGenerated.items.find(item => item.id === 'worktree')!.usage.state = 'unable_to_determine';
    const unknown = createTemporaryStorageMock(unknownGenerated); const scan = await unknown.scan(() => {});
    await expect(unknown.prepare(scan.scan_id, ['worktree-target'])).rejects.toThrow('complete use observations');
  });

  it('consumes exact consent once and reports preview moves without freeing bytes', async () => {
    const mock = createTemporaryStorageMock(); const inventory = await mock.scan(() => {});
    const draft = await mock.prepare(inventory.scan_id, ['detached-whole']);
    await expect(mock.execute(draft.id, { ...consent, accept_unknown_usage: false })).rejects.toThrow('Confirm');
    await expect(mock.execute(draft.id, consent)).rejects.toThrow('already used');
    const accepted = await mock.prepare(inventory.scan_id, ['worktree-target']);
    expect(accepted).toMatchObject({ has_unknown_usage: false, has_whole_folders: false });
    const result = await mock.execute(accepted.id, { ...consent, accept_source_loss: false, accept_unknown_usage: false });
    expect(result.moved_count).toBe(1); expect(result.items[0].message).toContain('No files were changed');
  });

  it('expires and cancels drafts and preserves cancelled scan qualification', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-01T00:00:00Z'));
    const mock = createTemporaryStorageMock(); const scan = await mock.scan(() => {});
    const draft = await mock.prepare(scan.scan_id, ['detached-whole']);
    await mock.cancelExecution(draft.id);
    await expect(mock.execute(draft.id, consent)).rejects.toThrow('already used');
    const stale = await mock.prepare(scan.scan_id, ['detached-whole']);
    vi.advanceTimersByTime(301_000);
    await expect(mock.execute(stale.id, consent)).rejects.toThrow('expired');
    vi.advanceTimersByTime(600_000);
    await expect(mock.prepare(scan.scan_id, ['detached-whole'])).rejects.toThrow('inventory expired');
    const cancelled = createTemporaryStorageMock();
    const partial = await cancelled.scan(event => { if (event.type === 'started') void cancelled.cancelScan(event.scan_id); });
    expect(partial).toMatchObject({ partial: true, cancelled: true, observed_allocated_bytes: 0 });
    await expect(cancelled.prepare(partial.scan_id, ['detached-whole'])).rejects.toThrow('inventory expired');
  });

  it('chooses one exact scope per folder and explains uncertainty without implied eligibility', () => {
    const store = new TemporaryReviewStore(structuredClone(temporaryStorageFixture));
    expect(store.selectedIds).toEqual([]);
    store.choose('worktree', 'worktree-whole', true); store.choose('worktree', 'worktree-target', true);
    expect(store.selectedIds).toEqual(['worktree-target']);
    const rendered = render(TemporaryStorageView, { props: { onBack: () => {}, initialResult: temporaryStorageFixture } }).body;
    expect(rendered).toContain('Unable to determine use'); expect(rendered).toContain('No use detected'); expect(rendered).toContain('In use');
    expect(rendered).toContain('Not estimated'); expect(rendered).toContain('Partial inventory');
    expect(rendered).toContain('separate from ready-to-clean cache totals'); expect(rendered).toContain('does not prove');
    expect(rendered).not.toContain('checked');
  });
});
