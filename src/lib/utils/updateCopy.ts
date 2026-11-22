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
