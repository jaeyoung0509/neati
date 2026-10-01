# Cross-window scan publication and neati motion — #388

Executed October 1, 2026 on macOS 27.0.1 (26A434), arm64.
Base: `develop@70c0c62ac6681b1697c7fb428326cb6d6dd234b7`.
Version **0.3.93 → 0.3.94**, synchronized once with `just bump-patch`.

## Confirmed race and regression coverage

A second webview could cancel the first worker and replace its ScanStore
generation before acquiring a read budget. The concurrent read gate then let
both workers run; the older result was correctly refused at publication. The
[baseline start regression](race-baseline.txt) and
[continuation regression](continuation-baseline.txt) fail against the original
implementation. These are controlled application-service fixtures, not evidence
of which window triggered the owner's screenshot.

CleanupService now admits at most 18 scan sessions, acquires its private scan
lifecycle before a shared read slot, and keeps both guards and admission in the
actual blocking worker through publication. A queued scan does not consume the
remaining slot for unrelated reads. Pre-dispatch abandonment preserves current
authority; cancellation and panic retire the claimed lease and reported Stop
handle. Obsolete publication, one-shot continuation, freshness and incomplete
cleanup-authority rules remain enforced. Requests are queued, not deduplicated;
this is not a new scan-speed benchmark.

The [focused cleanup-service suite](cleanup-service-tests.txt) passes **20/20**,
including ten new regressions for concurrent starts, continuation/start ordering,
cancellation, panic, abandoned callers, read concurrency, bounded admission and
abandonment while awaiting read capacity. Fixtures use an empty catalog,
synthetic environment, controlled progress sinks and recording provider ports.
No real-user scan or cleanup was performed.

The existing Brave planner test was also isolated from the host's running apps
with the existing fake process and open-file ports. A synthetic running Brave
still produces a SafetyBoundary refusal; the idle case moves only the fixture
unit and preserves its Cookies and HTTP-cache siblings. Production safety
checks were not weakened.

## Executed local verification

| Check | Result |
| --- | --- |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 483 passed in 50 files |
| `pnpm build` | Passed |
| Cargo format, workspace check, all-target Clippy with `-D warnings` | Passed |
| `cargo test --offline --locked --workspace --no-fail-fast` | 1,302 passed, 6 existing ignored, 0 failed, 0 filtered; doc tests passed |
| `just check-architecture`, `just check-version` | Passed |
| Three binding/golden exports and tracked binding drift check | Passed; no drift |
| `pnpm icons:check` | Passed |
| `just build-fast` after final version and frontend changes | Passed |
| Bundled `Neati --doctor --json` | 14/14 passed |

[Command results and bundle facts](local-checks.json) record the executed commands
and timings. Both debug bundle version fields report **0.3.94**. The packaged
icon matches the tracked ICNS SHA-256; all **13** current generated JS/CSS asset
names are embedded in the executable. [Doctor's report](doctor.json) contains
the de-identified environment and self-check results.

The debug application window was not launched or installed. The installed
application remains **0.3.93**. No release, tag or signing claim is part of this
record. CI results are separate from these local checks.

## Mounted browser verification

Production Svelte components were mounted with synthetic data. Main captures
show **Preview data** and **v0.3.94**; Quick Panel captures explicitly identify
their mocked window and IPC. These records do not measure the owner's disk or
exercise native cleanup.

- [Layout](browser-layout-checks.json): **36/36** passed across Overview,
  Cleanup, Performance, Memory, AI Activity and Settings, both themes, at
  800×560, 960×660 and 1280×820; no horizontal overflow.
- [Interactions](browser-interaction-checks.json): **11/11** passed, including
  preparation, stopping, failed-scan read-only state and retry, keyboard path
  disclosure, immediate real readings and stable metric rectangles.
- [Main motion](main-motion-checks.json): **11/11** passed. The fixed handwritten
  glyph has two 2.4-second CSS loops; the five ambient curves use 7/9-second
  working and 18/22-second idle cadence. Stopping freezes them. Actual scrolling
  and browser document visibility remove offscreen/hidden motion; changed
  readings stay immediate and do not replay on return. Rapid bursts leave no
  animation queue. Actual browser reduced-motion produces static glyphs and
  values. The lower-right wave placement stays below count and elapsed text.
- [Quick Panel motion and geometry](quick-motion-checks.json): **7/7** passed.
  Scan/cleanup/error and provider-stream changes preserve mocked shell bounds,
  reserved gauge slots and footer placement with no extra resize requests.
  Mock window hiding releases the usage subscription and stops motion; hidden
  readings remain current and activation does not replay numbers.

All four completed browser records contain zero browser errors. The shared
observer's reduced-transparency branch is covered by unit tests; actual browser
media emulation and native OS preference behavior for that branch were not
executed. No native glass, tray positioning, native cancellation, native Quick
Panel bounds or Windows/Linux interactive behavior is established here. Those
acceptance tasks remain in #389, #376 and #380.

| Surface/state | Light | Dark |
| --- | --- | --- |
| Cleanup scanning, 960×660 | [Capture](scanning-light-960.png) | [Capture](scanning-dark-960.png) |
| AI provider refresh, 800×560 | [Capture](ai-loading-light-800.png) | [Capture](ai-loading-dark-800.png) |
| Quick Panel loading, 400 px | [Capture](quick-loading-light-400.png) | [Capture](quick-loading-dark-400.png) |

Additional captures: [preparing](preparing-light-960.png),
[stopping](stopping-light-960.png), [stopped](stopped-light-960.png),
[failure](failed-light-960.png), [working Overview](overview-working-light-800.png),
and [ready Quick Panel](quick-ready-dark-400.png).
[Capture metadata](keyframes.json) retains each version, size and timestamp.

Rebuild the isolated fixtures with:

```sh
node scripts/build_ui_polish_validation.mjs /private/tmp/neati-ui-388-fixture
node scripts/build_quick_panel_validation.mjs /private/tmp/neati-quick-388-fixture
```

Their standalone `ui-polish.html` and `quick-panel.html` expose typed validation
drivers for these synthetic transitions; the validation entry points are not
shipped product flows.

The separate [source-comparison record](../../evidence/cleanup-open-source-audit-2026-10-01.md)
links the six new cleanup improvement issues. It contains source findings,
not additional cleanup implementation or reclaim/performance measurements.
