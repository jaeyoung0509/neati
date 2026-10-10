# Permanent deletion validation — 0.3.107

Executed October 9, 2026 on macOS 27.0.1 (26A434), Apple Silicon. Issue [#416](https://github.com/jaeyoung0509/neati/issues/416). Version transition: 0.3.106 → 0.3.107.

## Local checks

| Check | Result |
| --- | --- |
| `cargo check --workspace` | Passed |
| `cargo test --workspace` | Passed: 1,441 tests; 17 existing ignored/export/profiling cases |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `just check-architecture` | Passed |
| `just generate-bindings` | Passed; updated IPC and capabilities exported |
| `just check-version` | Passed; all synchronized manifests 0.3.107 |
| `pnpm check` | Passed: zero errors/warnings |
| `pnpm test -- --run` | Passed: 58 files / 531 tests |
| `pnpm build` | Passed; production CSS utility check passed |
| `just build-fast` | Passed; debug `.app` includes production frontend |
| `cargo check -p neati-platform --tests --target x86_64-pc-windows-msvc` | Passed; compile evidence only |
| Windows-target platform Clippy, all targets, warnings denied | Passed; compile evidence only |

Regression coverage includes permanent default for Rebuild, catalog refusal of Trash, the real reviewed-file native executor, stale root identity, post-inspection entry substitution, final owner-use refusal, safe link unlinking, read-only directories and explicit partial-removal failure. Existing plan replay/expiry, source-loss/unknown-use consent, stopped-browser, provenance and protected-state tests remain active. All mutations use disposable fixtures.

The debug bundle reports both `CFBundleShortVersionString` and `CFBundleVersion` as 0.3.107. Its `icon.icns` matches `src-tauri/icons/icon.icns`: SHA-256 `6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`. The bundle was built but **not installed or launched**; no installed/running app was replaced.

## Browser visual evidence

Production Svelte components were mounted using `scripts/build_ui_polish_validation.mjs` and deterministic browser IPC mocks, then inspected with agent-browser. Every recorded page was checked at 800×560, 960×660 and 1280×800, in light/dark themes with reduced motion. Adjacent `*-matrix.json` files record zero horizontal document overflow and visible enabled action controls after scrolling the existing review region into view. Reviews keep Cancel and expiry text. Whole temporary folders retain the source/Git/session-loss acknowledgement; the mock confirm action stays disabled until it is checked. The temporary dialog has no stale Trash-move/recovery promise. Browser confirmation previews data and performs no real deletion.

| Surface | Light / minimum window | Dark / desktop |
| --- | --- | --- |
| Ordinary cleanup | [800×560](cleanup-light-800.png) | [1280×800](cleanup-dark-1280.png) |
| Large Files review | [800×560](large-files-light-800.png) | [1280×800](large-files-dark-1280.png) |
| App uninstall review | [800×560](app-uninstall-light-800.png) | [1280×800](app-uninstall-dark-1280.png) |
| Developer Artifacts review | [800×560](developer-artifacts-light-800.png) | [1280×800](developer-artifacts-dark-1280.png) |
| Temporary Folder review | [800×560](temporary-folders-light-800.png) | [1280×800](temporary-folders-dark-1280.png) |

## Limits

Windows native execution and installer behavior were not run on this macOS host. No real user directories or native UI deletion actions were exercised. Cross-compilation and browser screenshots do not establish Windows runtime correctness. PR CI status is reported separately in GitHub; local checks do not claim pending CI success. Permanent removal may be partial on error and is not secure erasure or a guaranteed physical free-space delta.
