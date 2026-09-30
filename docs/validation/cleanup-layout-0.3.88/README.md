# Cleanup guidance density — issue #362

September 30, 2026 · neati 0.3.87 → 0.3.88 · macOS 27.0.1 (26A434), arm64.

The reported Cleanup screen repeated coverage, owner-use and permission explanations
before users could reach cache selection. The summary now shows ready bytes once,
keeps a visible partial-scan qualification and places conditional amounts beside
their review actions. `Manage access` and `Scan details` start closed. The complete
item inventory follows category selection, groups review/owner actions before
kept/unverified observations, and retains its records rather than presenting them
as available cleanup actions. `Review items` opens, scrolls to and focuses that
inventory. The cleanup toolbar remains last in keyboard order and visible while
scrolling. Access progress and errors stay visible outside the closed help.

The existing backend eligibility, selected-byte accounting, app-shutdown review,
provider confirmation and one-shot plan rules are unchanged. Native glass and
branding were not modified.

## Visual and interaction evidence

Live browser preview used an isolated agent-browser profile, application IPC mocks
and a synthetic partial scan with 791 total records / 790 detail rows: ready,
running-owner, reviewed, unestimated and protected/unverified observations. The
synthetic overlap range and gap counts reproduce the reported density; they are
not another measurement of the user's disk. No cleanup action was invoked.

All eleven retained captures were visually inspected. Six complete application
layouts cover light/dark at 800×560, 960×660 and 1280×800 with reduced motion.
The minimum window shows the first category name/amount and cleanup action; long
category metadata may continue beneath the sticky toolbar and remains reachable
by scrolling. No horizontal overflow occurred. Expanded captures were scrolled
to show their requested details.

| Evidence | Light | Dark |
| --- | --- | --- |
| Minimum window | [800×560](mixed-light-800.png) | [800×560](mixed-dark-800.png) |
| Default window | [960×660](mixed-light-960.png) | [960×660](mixed-dark-960.png) |
| Expanded window | [1280×800](mixed-light-1280.png) | [1280×800](mixed-dark-1280.png) |

- [Access instructions](access-help-dark-960.png): Enter opens, Space closes;
  actual-bundle instructions, possible file-permission causes and the absence of
  administrator authority remain available. Tabbing to Open System Settings
  scrolls the control above the sticky toolbar.
- [Scan details](scan-details-dark-960.png): keyboard disclosure retains typed
  reason counts, overlap qualification and unknown-estimate semantics.
- [Item details](item-details-dark-960.png): review action opens/focuses/scrolls;
  all 790 rows remain, grouped into three owner/review actions and 787 retained
  observations. Enter closes the disclosure.
- Static current-source component fixtures additionally cover initial, verified
  empty, unavailable, privacy refusal, ready, partial and retained states across
  both themes and all three widths: 42 checks. Unknown results retain a dash;
  verified empty results may show zero. [Empty](empty-light-960.png) and
  [unavailable](unavailable-dark-800.png) captures show that distinction.

Recorded results: [live checks](browser-checks.json),
[static checks](static-layout-checks.json).

Component fixtures can be reproduced after `pnpm build`:

```sh
node scripts/render_cleanup_validation.mjs /private/tmp/neati-cleanup-layout-fixtures
python3 scripts/capture_cleanup_validation.py /private/tmp/neati-cleanup-layout-fixtures
```

The renderer also writes `mixed-fixture.json`. Live checks loaded that fixture
into the existing scan store in a separate Vite browser preview and used the
existing settings store to apply themes. No validation hook is shipped in the app.
Static controls are not interaction evidence; the keyboard/scroll checks above
ran against the mounted application.

## Local verification

- `cargo check --workspace`: passed.
- `cargo test --workspace`: 1,262 passed, five existing ignored, zero filtered.
- `just lint-rust`: format and Clippy passed.
- `just check-architecture`, `just check-version`: passed.
- `pnpm check`: zero errors/warnings.
- `pnpm test -- --run`: 467 passed in 48 files. After the final review-scroll
  adjustment, the affected Storage suite passed again: 39 tests.
- `pnpm build` and final `just build-fast`: passed after the patch bump and
  frontend changes; the bundle contains the current frontend.
- Bundle short/build versions are both 0.3.88; the packaged ICNS matches the
  tracked icon. `pnpm icons:check`: 61 assets/registry entries verified.
- The bundled executable's `--doctor`: 14/14 checks passed.

CI status is reported separately on the PR. The installed/running application
was not replaced. Browser captures do not prove native glass composition,
unsigned-bundle grant/deny/revoke/relaunch/update behavior or Windows runtime UI.
Those platform checks remain unverified in this batch.

## Remaining issue scope

Merged #374 delivered installed CocoaPods 1.16.2/system Ruby 2.6.10 validation,
corrected its complete Pods-child command scope and expanded the recorded
Conda/mise version set. This PR reconciles that progress with the remaining scope
in [the ledger](../../CLEANUP_362_SCOPE.md). #362 stays open for native permission
transitions, additional runtime compatibility, Android SDK/OrbStack/browser and
Corepack owner contracts, further SwiftPM scopes, database/index and unmatched
store decisions, and current-user system access/lifecycle evidence. The latest
Mole dry-run's unmatched Google Updater/OpenCode stores remain research leads.
Xcode #350 stays on the owner's setup hold; CLI #335, paid signing and privileged
helper mutation remain deferred.
