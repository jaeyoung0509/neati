import type { PlanPreview, PlanTargetPreview, ScanItem } from '../models/types';

export function summarizeCleanupReview(plan: PlanPreview, items: ScanItem[] = []) {
  const byId = new Map(items.map(item => [item.id, item]));
  const groups = new Map<string, {
    label: string; consequence: string; mode: PlanTargetPreview['mode'];
    targets: PlanTargetPreview[]; bytes: number; unestimated: number;
  }>();
  for (const target of plan.targets) {
    const item = byId.get(target.item_id);
    const label = item?.cache_metadata?.provider?.trim()
      || item?.ownership?.owner?.trim() || 'Other selected items';
    const consequence = item?.cache_metadata?.consequence?.trim() || '';
    // Presentation only: never infer the execution mode from risk or a path.
    const key = JSON.stringify([label, consequence, target.mode, target.risk]);
    const group = groups.get(key) ?? {
      label, consequence, mode: target.mode, targets: [], bytes: 0, unestimated: 0,
    };
    group.targets.push(target);
    group.bytes += target.expected_bytes;
    group.unestimated += Number(target.expected_bytes === 0);
    groups.set(key, group);
  }
  const values = [...groups.values()].sort((a, b) => b.bytes - a.bytes);
  return {
    groups: values,
    unestimated: values.reduce((sum, group) => sum + group.unestimated, 0),
    deletedBytes: values.filter(group => group.mode === 'permanent_delete').reduce((sum, group) => sum + group.bytes, 0),
    trashBytes: values.filter(group => group.mode === 'trash').reduce((sum, group) => sum + group.bytes, 0),
  };
}
