import type {
  OneButtonActivity,
  OneButtonItem,
  OneButtonOutcome,
  OneButtonPreview,
  ScanItem,
} from '../../models/types';

const GIB = 1024 * 1024 * 1024;

/** Idle, verified build output the preview's saved project folder holds. */
const projectFixtures: OneButtonItem[] = [
  {
    reference: 'project:7f3a1c90b2d4',
    source: 'project',
    name: 'clean1',
    kind: 'cargo target',
    path: '/Users/mock/Myproject/clean1/target',
    bytes: 30.7 * GIB,
  },
  {
    reference: 'project:1b8e44f0a6c2',
    source: 'project',
    name: 'android-app',
    kind: 'gradle build',
    path: '/Users/mock/work/android-app/build',
    bytes: 4.4 * GIB,
  },
];

/**
 * Browser-preview fixture for the one-button Clean commands (#500). The
 * caller hands in the cache items the mock scan marked ready; this file only
 * shapes them like the backend preview and keeps the one-shot id contract.
 */
export function createOneButtonCleanMock(readyCacheItems: () => ScanItem[]) {
  const held = new Map<string, OneButtonPreview>();
  let next = 0;

  return {
    async preview(onEvent: (event: OneButtonActivity) => void): Promise<OneButtonPreview> {
      onEvent({ type: 'scanning', location: 'Project folders' });
      const caches: OneButtonItem[] = readyCacheItems().map((item) => ({
        reference: item.id,
        source: 'cache',
        name: item.name,
        kind: item.category,
        path: item.path,
        bytes: item.disposition?.cleanable_bytes ?? 0,
      }));
      const items = [...caches, ...projectFixtures];
      const cacheBytes = caches.reduce((total, item) => total + item.bytes, 0);
      const projectBytes = projectFixtures.reduce((total, item) => total + item.bytes, 0);
      next += 1;
      const preview: OneButtonPreview = {
        id: `00000000-0000-4000-8000-${String(next).padStart(12, '0')}`,
        items,
        total_bytes: cacheBytes + projectBytes,
        cache_bytes: cacheBytes,
        project_bytes: projectBytes,
        worktree_bytes: 0,
        review_items: 2,
        review_bytes: 1.4 * GIB,
        project_folders: 1,
      };
      held.set(preview.id, preview);
      return preview;
    },

    async execute(
      previewId: string,
      onEvent: (event: OneButtonActivity) => void
    ): Promise<OneButtonOutcome> {
      const preview = held.get(previewId);
      held.delete(previewId);
      if (!preview) {
        throw new Error('This preview is no longer current. Check again before cleaning.');
      }
      preview.items.forEach((item, index) =>
        onEvent({ type: 'removing', name: item.name, index, total: preview.items.length })
      );
      return {
        reclaimed_bytes: preview.total_bytes,
        removed: preview.items.length,
        skipped: 0,
        failed: [],
      };
    },
  };
}
