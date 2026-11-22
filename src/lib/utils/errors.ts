/**
 * Text for something that was thrown. A `catch` variable is `unknown`: IPC
 * rejections arrive as strings, browser failures as `Error`, and a rejected
 * promise can carry anything.
 *
 * Returns `String(error)` (so an `Error` reads "Error: …", as before), or
 * `fallback` when nothing usable was thrown.
 */
export function describeError(error: unknown, fallback = ''): string {
  if (error === null || error === undefined) return fallback;
  return String(error) || fallback;
}

/** Like {@link describeError}, but prefers a bare `message` when there is one. */
export function describeErrorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.length > 0) return message;
  }
  return String(error);
}
