import type { UpdateCheck } from '../models/types';

/** One status sentence per backend outcome; the backend owns the reasons. */
export function updateStatusCopy(result: UpdateCheck | null): string {
  if (!result) return '';
  switch (result.status) {
    case 'up_to_date':
      return `neati ${result.current_version} is the newest release.`;
    case 'available':
      return `neati ${result.version} is available (you have ${result.current_version}).`;
    case 'unavailable':
      return result.reason;
    case 'failed':
      return result.message;
  }
}

/**
 * The install command downloads, verifies and replaces neati in one backend
 * call without step events. The busy button names the phase; this line says
 * how it ends rather than guessing which step is running.
 */
export const UPDATE_INSTALL_PHASE = 'neati restarts once the signed update is installed';
