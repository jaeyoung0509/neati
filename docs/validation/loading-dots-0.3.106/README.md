# Loading dots validation — v0.3.106

Evidence for [#411](https://github.com/jaeyoung0509/neati/issues/411), with one
patch transition from **0.3.105 to 0.3.106**. Recorded on October 2, 2026, on
macOS 27.0.1, build 26A434.

The main Cleanup scan-progress card retains handwritten scanning feedback.
Its header action, Overview, AI Activity, other busy actions and the Quick
Panel use three staggered bouncing dots. Quick Panel loaders sit beside their
status sentence, restoring the compact layout.

## Visual evidence

These captures mount production Svelte components in Chromium with synthetic
data and mocked APIs. They do not perform native scans, cleanup or provider
commands, and do not establish native glass or WKWebView behavior.

- [Cleanup, light, 960×660](cleanup-light-960.png): one handwritten scan word
  and a three-dot header action. The word's complete phase is frozen for the still.
- [Cleanup, light, 800×560](cleanup-light-800.png): the narrow main window.
- [Cleanup, dark, 1440×900](cleanup-dark-1440.png): the same loader hierarchy;
  the word's complete phase is frozen for the still.
- [Quick Panel, light, 360×800](quick-loading-light-360.png): inline cleanup
  and provider loading dots.
- [Quick Panel, dark, 320×800](quick-cleaning-dark-320.png): cleaning and
  provider loading at the narrow panel width.

## Browser checks

- [Main-window checks](main-browser-checks.json): seven cases passed.
  Cleanup keeps exactly one handwritten loader and uses dots in its header.
  All checked window sizes avoid horizontal overflow. SVG centre/fill-opacity
  samples show the staggered dot motion without transform layers. Overview and
  AI Activity use dots. Reduced motion stops both variants, and stopping a
  scan retains a static word and disabled Stop control.
- [Quick Panel checks](quick-browser-checks.json): 38 cases passed. The 36
  scan/provider combinations at widths 320, 360 and 400 retain their fitted
  height and use three-dot loaders throughout. The checked heights are 637 px
  at 320 px width and 625 px at 360/400 px width. Hidden panels release the
  provider subscription and stop loader motion; reduced motion stops it too.
- [Component checks](component-browser-checks.json): two cases passed.
  All three dot sizes remain decorative. An example action stays 157.16 px
  wide across idle/busy states; inactive and reduced-motion indicators are static.

## Local verification

Final checks and bundle details are recorded in
[local-verification.json](local-verification.json).

- `cargo check` and `cargo test`: 1,432 Rust tests passed, 17 ignored;
  documentation tests passed.
- `pnpm check`: zero errors and zero warnings.
- `pnpm test -- --run`: 58 files, 531 tests passed.
- `pnpm build`: Vite and Tailwind utility verification passed.
- Rust formatting, Clippy, `just check-architecture` and `just check-version` passed.
- `just build-fast` builds the debug app with the current frontend.

The installed/running **0.3.105** app was not replaced or relaunched. Remote CI
is reported separately in the PR. Native WKWebView paint/teardown, native glass
compositing and Windows behavior were not manually verified in this run.

## Reproduce the fixtures

From the repository root with dependencies installed:

```sh
node scripts/build_ui_polish_validation.mjs /private/tmp/neati-loading-ui
node scripts/build_quick_panel_validation.mjs /private/tmp/neati-loading-quick
pnpm dev --port 5178
```

Open `ui-polish.html`, navigate to Storage and call
`window.uiPolishValidation.scan('scanning')`. Overview uses that same synthetic
scan state; AI Activity can use `providerLoading(true)`. Open `quick-panel.html`
and use `window.quickPanelValidation.viewport(320)`, `scan('cleaning')` and
`transition('loading')`. The component gallery is at
`http://127.0.0.1:5178/src/test/fixtures/loading-variants.html`; its
`window.loadingValidation.configure({active: false})` exposes the inactive and
idle action states. Browser media emulation exercises reduced motion.
