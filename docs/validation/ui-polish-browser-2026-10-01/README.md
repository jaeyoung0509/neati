# UI polish #378 — final browser and local verification

Executed October 1, 2026 on macOS 27.0.1, build 26A434, Apple Silicon.
Final version **0.3.92 → 0.3.93**, synchronized with
`develop@48c1e7b3b6e5ba6bbbcb8cd0da11776af67d2821` through a merge and
`just bump-patch`. The conflicting version outputs were resolved together;
no production UI behavior changed during this synchronization.

This record supersedes the [initial restricted-environment record](../ui-polish-0.3.90/README.md).
The initial failures remain documented there. The fresh unfiltered run below
passes without changing or filtering their assertions.

## Executed local checks

| Check | Result |
| --- | --- |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 483 passed in 50 files |
| `pnpm build` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --offline --locked --workspace` | Passed |
| `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --offline --locked --workspace --no-fail-fast` | 1,292 passed, 6 existing ignored, 0 failed, 0 filtered; doc tests passed |
| Architecture and version checks | Passed |
| Three binding/golden exports and tracked drift check | Passed; no drift |
| `just build-fast` after the final version change | Passed |

[Command outcomes and bundle facts](local-checks.json) include the executed
commands and timings. The debug app reports 0.3.93 in both bundle version
fields. Its packaged icon matches the tracked icon SHA-256
`6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.
All 12 current generated JS/CSS asset names are embedded in the executable.
The installed/running application was not replaced or launched.

## Mounted browser evidence

The production Svelte components were mounted with deterministic synthetic
browser mocks. Every screenshot visibly marks **Preview data** and version
**0.3.93**. This validates the interface with mock data; no native IPC,
real-user cleanup, installation, signing, release or tag was performed.

- [Layout observations](browser-layout-checks.json): **36/36 passed** for
  Overview, Cleanup, Performance, Memory, AI Activity and Settings, in light
  and dark themes at 800×560, 960×660 and 1280×820. Each measurement verified
  the correct page title and no horizontal document/content overflow.
- [Interaction observations](browser-interaction-checks.json): **11/11 passed**.
  Preparation shows **0 items · 0s elapsed**, with Stop disabled until a scan
  ID exists. Scanning/stopping/post-cleanup checking remove stale actionable
  rows; failure disables prior selections and retry restores accepted results.
  Path disclosure and theme controls work from the keyboard with visible focus.
- Real CPU/memory updates appear immediately with one brief reveal per changed
  value and unchanged metric-card rectangles. Reduced motion produces zero
  numeric or ambient animations.
- [Quick Panel observation](quick-browser-checks.json): the built production
  quick route was captured at 400×740 using its default light browser-mock
  theme. The shell fits that viewport without horizontal overflow. This is
  browser component evidence, not a native-window fitting/lifecycle check.
- The completed main fixture run recorded **zero browser errors**. Its first
  attempt was interrupted after 27 actual passing rows by a CDP transport
  failure. [That interruption](browser-transport-interruption.json) is recorded
  separately; a fresh session completed the full 36+11 set.

[Main QA summary](qa-summary.json) records the run time, environment and
fixture scope. The 29 attached PNGs include 24 page/theme captures at 800 and
960 pixels, three scan states, keyboard focus and the Quick Panel. The
1280-pixel cases have recorded DOM measurements.

| Surface | Light | Dark |
| --- | --- | --- |
| Overview | [800](overview-light-800.png), [960](overview-light-960.png) | [800](overview-dark-800.png), [960](overview-dark-960.png) |
| Cleanup results | [800](cleanup-light-800.png), [960](cleanup-light-960.png) | [800](cleanup-dark-800.png), [960](cleanup-dark-960.png) |
| Performance | [800](performance-light-800.png), [960](performance-light-960.png) | [800](performance-dark-800.png), [960](performance-dark-960.png) |
| Memory | [800](memory-light-800.png), [960](memory-light-960.png) | [800](memory-dark-800.png), [960](memory-dark-960.png) |
| AI Activity | [800](ai-light-800.png), [960](ai-light-960.png) | [800](ai-dark-800.png), [960](ai-dark-960.png) |
| Settings | [800](settings-light-800.png), [960](settings-light-960.png) | [800](settings-dark-800.png), [960](settings-dark-960.png) |
| Quick Panel | [400](quick-light-400.png) | Not captured in this browser record |

Additional captures: [preparing](scan-preparing-light-800.png),
[stopping](scan-stopping-light-800.png), [failure](scan-failed-light-800.png),
and [keyboard focus](settings-dark-keyboard-800.png).

Reproduce the main fixture from this version with:

```sh
node scripts/build_ui_polish_validation.mjs /private/tmp/neati-ui-polish-final
```

Open its generated `ui-polish.html`; `window.uiPolishValidation` provides the
scan-state, theme and actual-value drivers. The browser fixture is a validation
entry, not a shipped application flow.

## Scope and follow-up

Native glass, native cancellation, native Quick Panel fitting/activation and
Windows desktop interactions were not exercised by these browser runs. The
existing native material and sizing owners are unchanged. These follow-ups
remain in #378 and #380; no platform parity is claimed.

The owner explicitly approved merging after conflict/version resolution
without awaiting refreshed CI results. CI is separate from this executed local
and browser record; the initial 0.3.90 proposal passed
[run 36800836424](https://github.com/jaeyoung0509/neati/actions/runs/36800836424).
