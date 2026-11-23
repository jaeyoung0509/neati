/**
 * The browser OAuth wait is owned by the backend: `OAUTH_TIMEOUT` in
 * `src-tauri/src/ai_providers/openrouter.rs` stops listening for the callback
 * after this long. The interface only names that limit; it never cancels the
 * wait itself. `providerSignIn.test.ts` pins the two values together.
 */
export const OPENROUTER_SIGN_IN_TIMEOUT_SECONDS = 180;

export type OpenRouterAction = 'connecting' | 'disconnecting';

/** `Finish sign-in in your browser. neati stops waiting after 3 min.` */
export function openRouterWaitHint(timeoutSeconds = OPENROUTER_SIGN_IN_TIMEOUT_SECONDS): string {
  return `Finish sign-in in your browser. neati stops waiting after ${Math.round(timeoutSeconds / 60)} min.`;
}
