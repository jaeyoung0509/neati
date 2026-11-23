# Busy states, part 3 — #491

Recorded on November 23, 2026, on macOS 27.0.1 (build 26A434) with headless
Google Chrome 155 against the browser preview (`vite`, mocked IPC), from the
`feature/491-busy-states-3` branch at version 0.3.134 (no version bump in this
branch; the batch PR owns it).

These captures render production Svelte components. Busy states were held by
setting the same store modules the app imports (for example
`usageStore.openRouterAction`, `awakeStore.pending`, `scanStore.cleanProgress`);
no native scan, cleanup, sign-in, install or Keep Awake command ran, and the
captures say nothing about native glass or WKWebView behaviour. Reduced motion
was emulated, so the dots are static.

- [Storage clean, light, 960×660](clean-progress-light-960.png) and
  [dark, 800×560](clean-progress-dark-800.png): `Removing npm cache · 3 of 12 · 9 s`;
  the bar counts finished targets only.
- [Large Files plan card, light, 960×660](large-files-plan-card-light-960.png)
  and [Apps plan card, light, 1200×800](apps-plan-card-light-1200.png): the
  shared `RemovalPlanCard`, with each view's own copy, countdown and
  permanent-removal wording; the Apps card uses the stacked compact layout.
- [Settings → Updates install, light, 960×660](update-install-light-960.png):
  both update buttons disabled, the install button busy, and elapsed seconds
  beside it. The install command reports no steps, so no step is named.
- [Keep Awake first read, light, 960×660](awake-checking-light-960.png):
  `Checking Keep Awake…` instead of `Normal sleep is active` before the state
  answers.
- [Keep Awake manual start, light, 960×660](awake-starting-light-960.png): all
  durations disabled, the chosen one says `Starting…`.
- [OpenRouter wait, light, 960×660](openrouter-waiting-light-960.png) and
  [dark, 800×560](openrouter-waiting-dark-800.png): `Waiting for OpenRouter… 13 s`,
  the backend's 3-minute limit, and a disabled, busy connect button.
- [AI accounts first read, dark, 800×560](ai-usage-checking-dark-800.png):
  `Checking account usage…` instead of "No AI account usage metadata".
