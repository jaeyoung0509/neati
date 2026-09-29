import type { EmptyTrashPreview, EmptyTrashResult } from '../../models/types';

export async function previewEmptyTrash(): Promise<EmptyTrashPreview> {
  return { plan_id: 'mock-home-trash', scope: '~/.Trash', items: ['Old installer.dmg', 'Discarded cache'], observed_bytes: 125829120, entry_count: 12 };
}
export async function executeEmptyTrash(_planId: string): Promise<EmptyTrashResult> {
  throw new Error('Emptying Trash is unavailable while neati is showing preview data.');
}
export async function cancelEmptyTrash(_planId: string): Promise<void> {}
