import { refusalForPreview } from '../api';
import { tauriExecuteEmptyTrash } from './tauri';
import type { EmptyTrashPreview, EmptyTrashResult } from '../models/types';
export async function emptyReviewedTrash(
  preview: EmptyTrashPreview,
  execute: (planId: string) => Promise<EmptyTrashResult> = tauriExecuteEmptyTrash,
  refusal: (action: string) => string | null = refusalForPreview,
): Promise<EmptyTrashResult> {
  const reason = refusal('Emptying Trash');
  if (reason) throw new Error(reason);
  if (!preview.plan_id || preview.entry_count === 0) throw new Error('Review non-empty Trash before confirming.');
  return execute(preview.plan_id);
}
