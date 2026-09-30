# Developer temporary cleanup and Cleanup UX — #369 / #370

Version **0.3.84 → 0.3.85**, one synchronized patch bump. Based on develop
`6a63b1af5498b93949a33dcc8f90b9753d8db3e0` (PR #368).

Validation ran on **September 30, 2026**, **macOS 27.0.1 (26A434), arm64**,
with Node **26.7.0** and agent-browser **0.27.1**. CI results belong to the PR;
the results below are local evidence.

## Delivered scope

The dedicated owner inventories only the exact default `node-compile-cache`
namespace under injected user/shared temporary roots. Only the recorded Node
26.7.0 arm64/V8-tag/current-user group format can move to Trash after positive
payload/header/checksum, ownership/access, stable identity, three-day whole-unit
inactivity and fresh process/open-handle checks. Other groups retain observed
bytes where measurable and cannot authorize removal. The prefix observer keeps
workspaces, session scratch and PR recovery artifacts advisory and does not
observe the exact namespace a second time. No whole-temp sweep is introduced.

Cleanup distinguishes verified empty scans from unknown/partial inspection,
stale results and retained-only inventory. Unknown totals use a dash; partial
ready totals explicitly cover checked locations. Empty/retained-only lists omit
unusable selection controls. Permission details start closed and remain keyboard
accessible; typed diagnostics choose the appropriate recovery path.

## Local checks

| Check | Result |
| --- | --- |
| `cargo check --workspace` | Passed |
| `cargo test --workspace` | 1,261 passed; 5 existing intentionally ignored exports/native smoke cases |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `just check-architecture` | Passed; core/platform boundaries preserved |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 465 passed across 48 files |
| `pnpm build` | Passed; production Tailwind utilities verified |
| `pnpm icons:check` | 61 assets/registry entries verified |
| `just generate-bindings` | All three exports passed; no tracked binding/golden drift |
| `just check-version`, `git diff --check` | Passed |
| `just build-fast` | Debug macOS `.app` bundle built with embedded frontend |

The generated bundle reports both version fields as **0.3.85**, executable
`Neati`, and `icon.icns`. Its packaged icon matches the tracked source byte for
byte (SHA-256 `6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`).
The Unix adapter test gate has an explicit reviewed hygiene exception because
its UID/mode/link/descriptor fixtures cannot run on Windows. The pure payload
and root-alias policies remain platform-independent tests.

## Visual evidence

The actual Svelte Cleanup view was rendered with production CSS and labeled
synthetic data. Seven scenarios × two themes × three viewport sizes
(**800×560, 960×660, 1280×800**) passed horizontal-overflow, reduced-motion and
state-copy checks. Captures follow the rendered content height, preserving
warnings and actions instead of adding a fixed blank canvas. All committed PNGs
were inspected. Machine-readable measurements are in
[browser-layout-checks.json](browser-layout-checks.json).

| Scenario | Evidence |
| --- | --- |
| Before first scan | [Light, 960](initial-light-960.png) |
| Verified empty | [Light, 960](empty-light-960.png), [dark, 960](empty-dark-960.png) |
| Inspection unavailable | [Dark, 800](unavailable-dark-800.png) |
| Protected-path access refusal | [Light, 800](privacy-light-800.png), [dark, 800](privacy-dark-800.png) |
| Mixed partial with verified Node cache | [Light, 960](partial-light-960.png), [dark, 960](partial-dark-960.png) |
| Retained workspace only | [Light, 960](retained-light-960.png) |
| Verified ready cache | [Dark, 1280](available-dark-1280.png) |
| Keyboard-expanded permission details | [Dark, 800](privacy-dark-keyboard-expanded-800.png) |
| Live browser preview shell, mock inventory | [960×660](browser-preview-960.png) |

Tab traversal reached the workflow selector, Scan Storage and permission
summary. Enter expanded the native details disclosure with a visible focus
ring. The live browser preview also showed visible keyboard focus on the sidebar
control. Static fixture buttons do not exercise IPC; the live shell uses the
existing browser mocks and explicitly displays Preview data.

To reproduce the component captures after a production build:

```sh
node scripts/render_cleanup_validation.mjs
python3 scripts/capture_cleanup_validation.py
```

The capture helper creates its own browser session/profile and closes it after
validation. Generated self-contained HTML is ignored rather than committing
copies of production CSS.

## Limits and installation status

Native Full Disk Access grant/deny/revoke/relaunch/upgrade transitions, native
WebView materials and Windows runtime behavior were not manually verified.
The new Node owner is macOS-only; unsupported formats remain advisory. Simulator
temporary data stays in #350; broader owner contracts stay outside this batch.

The bundle was built, **not installed**: the running `/Applications/neati.app`
was not replaced. No real user cache was cleaned, permission changed, PR merged,
release published or tag created.
