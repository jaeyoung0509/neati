/**
 * Where a view's first read stands (#491). A bare `isLoading` boolean is false
 * both before the first read starts and after it ends, so an empty list read as
 * a final "nothing found" before any answer arrived. The phase keeps those
 * apart: only `ready` may render an empty result as final.
 *
 * A later refresh keeps `ready` (the previous answer stays visible beside the
 * view's own refresh indicator); a failed refresh keeps it too, and the view
 * shows the error next to the answer it already has.
 */
export type LoadPhase = 'idle' | 'loading' | 'ready' | 'error';

/** No answer has arrived yet: an empty result is not final. */
export function awaitingFirstAnswer(phase: LoadPhase): boolean {
  return phase === 'idle' || phase === 'loading';
}

/** The phase while a read runs. */
export function loadStarted(phase: LoadPhase): LoadPhase {
  return phase === 'ready' ? 'ready' : 'loading';
}

/** The phase after a read failed. */
export function loadFailed(phase: LoadPhase): LoadPhase {
  return phase === 'ready' ? 'ready' : 'error';
}
