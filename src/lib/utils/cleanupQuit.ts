import type { CleanupQuitPreview, MemoryTerminationResult, ScanItem, ScanResult } from '../models/types';
import { refusalForPreview } from '../api';
import { isBulkSelectable } from './cleanup';

/** Each step uses fresh backend authority. Cancellation never starts another
 * quit or cleanup; an already sent quit request cannot be undone. */
export async function quitAndRescan(
  preview: CleanupQuitPreview,
  ports: {
    quit: (lease: string) => Promise<MemoryTerminationResult>;
    scan: () => Promise<ScanResult | null>;
    cancelled: () => boolean;
    progress: (message: string) => void;
  },
): Promise<ScanItem[]> {
  const refusal = refusalForPreview('Quitting apps');
  if (refusal) throw new Error(refusal);
  for (const app of preview.apps) {
    if (ports.cancelled()) return [];
    ports.progress(`Quitting ${app.name}…`);
    const result = await ports.quit(app.lease_id);
    if (result.outcome !== 'released') {
      throw new Error(`${app.name} is still running or changed. Quit it yourself and scan again.`);
    }
  }
  if (ports.cancelled()) return [];
  ports.progress('Checking caches again…');
  const scan = await ports.scan();
  if (!scan || scan.cancelled || ports.cancelled()) return [];
  const reviewed = new Set(preview.apps.flatMap(app => app.item_ids));
  return scan.categories.flatMap(category => category.items)
    .filter(item => reviewed.has(item.id) && isBulkSelectable(item));
}
