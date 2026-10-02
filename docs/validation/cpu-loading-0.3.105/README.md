# CPU history and handwritten status validation — v0.3.105

Evidence for issues [#408](https://github.com/jaeyoung0509/neati/issues/408) and
[#409](https://github.com/jaeyoung0509/neati/issues/409), with one patch transition
from **0.3.104 to 0.3.105**. Recorded on October 2, 2026; build host:
macOS 27.0.1, build 26A434.

The browser captures mount production Svelte components in Chromium with
synthetic data and mocked window/display APIs. CPU readings, storage operations
and provider usage in these fixtures are examples, not measurements or actions
on the user's machine.

## Visual evidence

| Evidence | Viewport and case |
| --- | --- |
| [CPU recording gaps](cpu-gaps-light-960.png) | Light, 960×660; separate runs across missing readings |
| [Narrow CPU history](cpu-gaps-light-800x560.png) | Light, 800×560; content scrolled to show the complete chart |
| [Continuous CPU history](cpu-continuous-dark-1440.png) | Dark, 1440×900 |
| [Empty CPU history](cpu-empty-dark-1440.png) | Dark, 1440×900; no artificial line |
| [Single CPU reading](cpu-single-light-800.png) | Light, 800×700; circular point |
| [Storage scan](storage-scanning-light-960.png) | Light, 960×660; handwritten `scanning` with operation text |
| [Light word gallery](word-options-light.png), [dark word gallery](word-options-dark.png) | 800×700; `loading`, `scanning`, `cleaning`, `working`, each at `xs`, `sm`, `md` |
| [Reduced-motion gallery](word-options-reduced-motion.png) | 800×700; complete static words |
| [Quick Panel loading](quick-loading-light-360.png) | Light, 360×800 |
| [Narrow Quick Panel loading](quick-loading-light-320.png) | Light, 320×800 |
| [Quick Panel cleaning](quick-cleaning-dark-400.png) | Dark, 400×800 |
| [Handwritten scan motion](handwritten-scan.webm) | Browser animation capture; inspect alongside the stroke/color checks below |

The CPU viewport check at 800×560 found no horizontal overflow. Loading stills
freeze the complete-word phase for readability; the WebM records live writing.
Browser captures do not establish native glass or WKWebView rendering behavior.

## Browser checks

- [CPU checks](cpu-browser-checks.json): **15/15 passed**. Fixed 0–100% scale,
  continuous/gapped/single/empty history, accessible descriptions, circular
  single point, and horizontal overflow.
- [Word checks](word-browser-checks.json): **20/20 passed**. Connected paths for
  every word/size, unique gradient IDs, actual stroke progression and color
  changes, fully readable flow mode, inherited ink color, inactive motion,
  and unchanged idle/busy action width.
- [Quick Panel checks](quick-browser-checks.json): **36/36 passed** at widths
  320, 360 and 400. Operation wording matches scanning/cleaning/loading;
  panel height remains **715 px** across the checked operation and provider
  loading states.
- [Reduced motion](reduced-motion.json): **13 words passed** the static-word
  check, including the action example.

## Local verification and bundle

All final local gates passed:

Version, bundle/icon hashes and embedded asset checks are recorded in
[local-verification.json](local-verification.json).

- `pnpm check`: zero errors and zero warnings.
- `pnpm test -- --run`: 58 files, 531 tests passed.
- `cargo check` and workspace `cargo test`: 1,432 tests passed, 17 ignored;
  Rust documentation tests passed.
- Rust formatting, Clippy, `just check-architecture`, and `just check-version`.
- `just build-fast`, including the final Vite build and Tailwind build checks.

The resulting debug `.app` reports version **0.3.105** in `Info.plist`.
Its packaged icon hash matches the source icon, and all seven current frontend
entry asset names are embedded in the debug executable. An initial concurrent
Rust documentation-test build hit a dependency-variant error; the serial rerun
passed.

This is local evidence. Remote CI status is reported separately in the PR.
The app was **not launched, installed, or used to replace the running app**.

## Reproduce the fixtures

From the repository root with dependencies installed:

```sh
node scripts/build_ui_polish_validation.mjs /private/tmp/neati-cpu-loading-ui
node scripts/build_quick_panel_validation.mjs /private/tmp/neati-cpu-loading-quick
```

Open the generated `ui-polish.html` and `quick-panel.html` in Chromium. The UI
fixture exposes `window.uiPolishValidation`: navigate to Performance → CPU,
then call `cpuHistory('gaps')` and `theme(false)`. The history cases are
`continuous`, `gaps`, `single`, and `empty`; `theme(true)` selects dark mode.
For the scan view, navigate to Storage and call
`scan('scanning')`.

The Quick Panel fixture exposes `window.quickPanelValidation`: use
`viewport(320)`, `scan('scanning')`, `transition('loading')`, and
`measurement()` to inspect the reserved bounds. Repeat at widths 360 and 400,
with scan states `ready`, `cleaning`, `refreshing`, and `stopping`, and with
the `fresh` provider transition.

For the word gallery, run `pnpm dev --port 5178` and open
`http://127.0.0.1:5178/src/test/fixtures/loading-variants.html`.
`window.loadingValidation.configure(...)` accepts `active`, `dark`,
`tone: 'brand' | 'ink'`, and `motion: 'write' | 'flow'`. Emulate
`prefers-reduced-motion: reduce` in the browser to review the static fallback.

## Verification limits

Native/manual device QA is owned by the user. WKWebView paint, visibility and
teardown behavior, native material compositing, and Windows behavior were not
verified in this run. These fixtures do not invoke native cleanup or provider
commands, and successful browser checks do not establish platform parity.
