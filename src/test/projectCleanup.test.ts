import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import ProjectBuildOutputCard from '../lib/components/ProjectBuildOutputCard.svelte';
import { ProjectCleanupStore, projectCleanupStore } from '../lib/stores/projectCleanup.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';

function stubPreviewSettings(projectFolders: string[]) {
  const values = new Map([['neati.settings', JSON.stringify({ project_folders: projectFolders })]]);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  });
}

function withProjectFolders<T>(folders: string[], body: () => T): T {
  const previous = settingsStore.settings;
  try {
    settingsStore.settings = { ...previous, project_folders: folders };
    return body();
  } finally {
    settingsStore.settings = previous;
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
  projectCleanupStore.phase = 'idle';
  projectCleanupStore.result = null;
  projectCleanupStore.selectedIds = [];
});

describe('one-button Clean with project build output (#494)', () => {
  it('scans saved folders and keeps only the backend-preselected build output selected', async () => {
    stubPreviewSettings(['/Users/mock/Myproject']);
    await withProjectFolders(['/Users/mock/Myproject'], async () => {
      const store = new ProjectCleanupStore();
      await store.refresh();
      expect(store.phase).toBe('ready');
      expect(store.items.length).toBeGreaterThan(0);
      expect(store.items.every((item) => item.selected_by_default)).toBe(true);
      expect(store.items.some((item) => item.kind === 'node_modules')).toBe(false);
      expect(store.selectedIds.length).toBe(store.items.length);
      expect(store.selectedBytes).toBeGreaterThan(0);

      const first = store.items[0];
      store.toggle(first.id, false);
      expect(store.selectedIds).not.toContain(first.id);
    });
  });

  it('does nothing without a saved projects folder', async () => {
    await withProjectFolders([], async () => {
      const store = new ProjectCleanupStore();
      await store.refresh();
      expect(store.phase).toBe('idle');
      expect(store.items).toEqual([]);
      expect(await store.clean()).toBeNull();
    });
  });

  it('removes the selected build output through the one-shot plan', async () => {
    stubPreviewSettings(['/Users/mock/Myproject']);
    await withProjectFolders(['/Users/mock/Myproject'], async () => {
      const store = new ProjectCleanupStore(() => null);
      await store.refresh();
      const before = store.items.length;
      const removal = await store.clean();
      expect(removal?.removed_count).toBeGreaterThan(0);
      expect(store.items.length).toBeLessThan(before);
      expect(store.phase).toBe('ready');
    });
  });

  it('refuses to delete while neati shows preview data', async () => {
    stubPreviewSettings(['/Users/mock/Myproject']);
    await withProjectFolders(['/Users/mock/Myproject'], async () => {
      const store = new ProjectCleanupStore();
      await store.refresh();
      expect(await store.clean()).toBeNull();
      expect(store.error).toBeTruthy();
      expect(store.items.length).toBeGreaterThan(0);
    });
  });

  it('renders the not-scanned, scanning and ready states', () => {
    withProjectFolders([], () => {
      expect(render(ProjectBuildOutputCard).body).toContain('Not scanned');
    });
    withProjectFolders(['/Users/mock/Myproject'], () => {
      projectCleanupStore.phase = 'scanning';
      projectCleanupStore.activity = 'Measuring clean1 · Rust build output';
      expect(render(ProjectBuildOutputCard).body).toContain('Measuring clean1 · Rust build output');
    });
  });
});
