import type { OneButtonActivity, OneButtonOutcome } from '../models/types';

/** One line for what the one-button use case is doing right now. */
export function describeOneButtonActivity(activity: OneButtonActivity): string {
  switch (activity.type) {
    case 'scanning':
      return `Checking ${activity.location}…`;
    case 'measuring':
      return `Measuring ${activity.project} · ${activity.kind}…`;
    case 'removing':
      return activity.total > 1
        ? `Removing ${activity.name} · ${activity.index + 1} of ${activity.total}`
        : `Removing ${activity.name}…`;
    case 'checking':
      return `Checking ${activity.source}…`;
  }
}

export type OneButtonOutcomeState = 'success' | 'partial' | 'failed';

/** Success, partial (something failed but something was removed), or failed. */
export function oneButtonOutcomeState(outcome: OneButtonOutcome): OneButtonOutcomeState {
  if (outcome.failed.length === 0) return 'success';
  return outcome.removed > 0 ? 'partial' : 'failed';
}
