import type { AutomaticCleanStatus, AutomaticCleanWait } from '../models/types';
import { formatBytes, formatTimeAgo } from './format';

function waitReason(wait: AutomaticCleanWait): string {
  switch (wait) {
    case 'disabled':
      return 'Off.';
    case 'not_due':
      return 'Not due yet.';
    case 'on_battery':
      return 'Waiting for AC power.';
    case 'person_active':
      return 'Waiting until you are away from the Mac.';
    case 'conditions_unknown':
      return 'Waiting: power or activity could not be read.';
    case 'running':
      return 'Running now.';
    default:
      return '';
  }
}

/** One line for Settings: what the last run did, and what it is waiting for. */
export function automaticCleanSummary(status: AutomaticCleanStatus, nowMs: number = Date.now()): string {
  if (!status.available) return 'Automatic Clean is available on macOS only.';
  const parts: string[] = [];
  const run = status.last_run;
  if (run) {
    if (run.error) {
      parts.push(`Last run ${formatTimeAgo(run.at)} stopped: ${run.error}`);
    } else if (run.removed === 0 && run.failed === 0) {
      parts.push(`Last run ${formatTimeAgo(run.at)} found nothing to remove.`);
    } else {
      parts.push(
        `Last run ${formatTimeAgo(run.at)} removed ${run.removed} ${run.removed === 1 ? 'item' : 'items'} (${formatBytes(run.reclaimed_bytes)})${run.failed > 0 ? `; ${run.failed} could not be removed` : ''}.`
      );
    }
  } else if (status.settings.enabled) {
    parts.push('Has not run yet.');
  }
  if (status.settings.enabled && status.next_due_at && status.next_due_at * 1000 > nowMs) {
    const days = Math.ceil((status.next_due_at * 1000 - nowMs) / 86_400_000);
    parts.push(`Next run in about ${days} ${days === 1 ? 'day' : 'days'}.`);
  } else if (status.waiting && status.waiting !== 'disabled' && status.waiting !== 'not_due') {
    parts.push(waitReason(status.waiting));
  }
  return parts.join(' ') || 'Off.';
}
