import type { RemovalPlanPreview } from '../models/types';
import { formatBytes } from './format';

/** A plan's allocated size, `≥`-qualified when part of it could not be measured. */
export function planSizeLabel(plan: Pick<RemovalPlanPreview, 'allocated_size' | 'size_is_lower_bound'>): string {
  return `${plan.size_is_lower_bound ? '≥ ' : ''}${formatBytes(plan.allocated_size)}`;
}

/** `1 item` / `3 items`, with the caller's noun. */
export function countLabel(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

/** A one-shot plan reads as expiring once this many seconds remain. */
export const PLAN_EXPIRING_SOON_SECONDS = 60;
