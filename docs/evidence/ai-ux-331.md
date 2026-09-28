# AI Activity task navigation and on-demand diagnostics (#331)

## Design decision and reference review

Reviewed official public product/workflow documentation on 2026-09-28:

- [Cleaner One Pro](https://cleanerone.trendmicro.com/en-ca/cleaner-one-pro-for-mac/)
  presents scan, storage inspection and application management as user tasks.
- [CleanMyMac Smart Care](https://macpaw.com/support/cleanmymac/knowledgebase/smart-care)
  describes reviewing individual task details from a scan summary.

Our application of these patterns: lead with what the user can do; move
implementation diagnostics behind an explicit disclosure. This is a documentation
review, not a hands-on comparison of those applications. No third-party code,
assets, deletion rules or speed claims were imported.

AI Activity now exposes Usage and Projects. Supported-tool detection lives under
Settings → Diagnostics & Privacy Logs. It is built in, not a plugin installation
surface. Each tool has one status and optional technical details. Missing
detection does not prove a tool is uninstalled; a process does not prove work or
token use. The old page-wide “Local only” badge was removed because account
usage and local observations have different sources; existing per-provider
source labels remain authoritative.

Control Center was inspected and retained: it owns budget/manual-usage settings
and advisory controls. No cleanup authorization, provider collection preferences,
native glass or backend API changed.

## Verification

Version 0.3.70 → 0.3.71. Browser preview with deterministic mock data, Chromium
149.0.7827.55 on macOS 27.0 (26A428), 2026-09-28. Reduced motion enabled.
Light/dark rendering checked at 800×560, 960×660 and 1440×900 for AI Activity
and expanded diagnostics. Browser preview does not validate native translucency.

- ArrowRight activates Projects and wraps to Usage, moving keyboard focus.
- Enter opens/closes the native diagnostic disclosure and opens technical details.
- Instrumented the existing browser-preview store methods: Settings entry with
  the disclosure closed produced 0 snapshot refreshes / 0 integration lookups;
  opening produced 1 / 1; explicit Refresh produced 2 / 2; closing removed the
  panel without increasing either count; reopening produced 3 / 3.
- No added polling. Closing does not cancel an already in-flight shared request.
- No horizontal overflow at the checked sizes, including expanded details at 800px.
- No browser page errors reported. No real legacy marker or user data was removed.
- Vitest: 419 passed / 43 files, including diagnostic empty/loading/error/partial
  and stale states, initially absent diagnostic content and two-tab navigation.
- `pnpm check`: zero errors/warnings. `pnpm build`: passed.
- `cargo check`: passed. Final isolated `cargo test --quiet`: 1,152 passed,
  4 existing ignored. An earlier run failed during rustdoc while a packaging
  build overlapped; the entire suite was rerun successfully after it finished.
- `just check-architecture`, `pnpm icons:check`, `git diff --check`: passed.
- `just build-fast`: passed. Bundle short/build version both 0.3.71. Packaged
  icon matches source SHA-256
  `e063044c3c47a0e6bd01ac5b41ca515ab02d42fa72997490f1e7f67b62c9a225`.

No installed/running app replacement. Windows runtime UI and native macOS glass
were not exercised. CI results are separate from these local checks.

### Visual evidence

| View | 800px light / dark | 960px light / dark | 1440px light / dark |
| --- | --- | --- | --- |
| AI Activity | [light](ai-ux-331/activity-800-light.png) / [dark](ai-ux-331/activity-800-dark.png) | [light](ai-ux-331/activity-960-light.png) / [dark](ai-ux-331/activity-960-dark.png) | [light](ai-ux-331/activity-1440-light.png) / [dark](ai-ux-331/activity-1440-dark.png) |
| Detection | [light](ai-ux-331/detection-800-light.png) / [dark](ai-ux-331/detection-800-dark.png) | [light](ai-ux-331/detection-960-light.png) / [dark](ai-ux-331/detection-960-dark.png) | [light](ai-ux-331/detection-1440-light.png) / [dark](ai-ux-331/detection-1440-dark.png) |

[Expanded technical details, 800px dark](ai-ux-331/details-800-dark.png).

## Next optimization work (proposals, not implemented or benchmark results)

1. Measure scan latency and peak CPU/RSS separately from cleanup results on
   reproducible fixtures. Report cold/warm runs and partial-access conditions.
2. Continue the #329 ecosystem cache gap ledger using independent owner-specific
   safety tests; discovery alone must never grant deletion permission.
3. Compare verified removed bytes, recoverable movement and free-space delta
   separately. A larger headline total is not proof of better cleanup.
4. Review the Overview-to-action journey using task completion and error recovery,
   without inventing a health score, automatic memory purge or destructive action.
