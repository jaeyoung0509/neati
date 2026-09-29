import { describe, expect, it, vi } from 'vitest';
import { emptyReviewedTrash } from '../lib/utils/emptyTrash';
import type { EmptyTrashPreview, EmptyTrashResult } from '../lib/models/types';
const preview: EmptyTrashPreview = {plan_id:'one-shot',scope:'/fixture/.Trash',items:['old.dmg'],observed_bytes:4096,entry_count:1};
describe('Trash confirmation boundary', () => {
  it('preview mode never invokes a destructive adapter',async () => {
    const execute=vi.fn();
    await expect(emptyReviewedTrash(preview,execute,()=> 'Unavailable in preview data')).rejects.toThrow('Unavailable in preview data');
    expect(execute).not.toHaveBeenCalled();
  });
  it('sends only the backend plan ID and preserves partial outcomes',async () => {
    const result: EmptyTrashResult={removed_bytes:0,removed_entries:0,cancelled:true,items:[{path:'old.dmg',removed:false,message:'Entry changed'}]};
    const execute=vi.fn().mockResolvedValue(result);
    expect(await emptyReviewedTrash(preview,execute,()=>null)).toEqual(result);
    expect(execute).toHaveBeenCalledExactlyOnceWith('one-shot');
  });
  it('empty and missing reviews cannot invoke deletion',async () => {
    const execute=vi.fn();
    await expect(emptyReviewedTrash({...preview,entry_count:0},execute,()=>null)).rejects.toThrow('Review non-empty');
    await expect(emptyReviewedTrash({...preview,plan_id:''},execute,()=>null)).rejects.toThrow('Review non-empty');
    expect(execute).not.toHaveBeenCalled();
  });
});
