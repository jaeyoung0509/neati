# neati identity and typography verification

Date: 2026-09-28. Host: macOS 27.0 (26A428), Apple Silicon.
Version: 0.3.73 → 0.3.74. Base: `37b83be`, branch
`feature/338-neati-identity-polish`. Implements #338.

## Final owner direction

One neati identity only: no old-name detection, aliases, migration or installer
branches. This supersedes the initial issue request to retain old-app protection
and the earlier compatibility plan. Other applications and stored user data are
not touched. Repository origin now points to `jaeyoung0509/neati`.

The mark, wordmark and packaged icon are unchanged. Crate directories, package
names, library/types/wire spellings, persistence/credential/log names, asset keys,
CI, release artifacts, WinGet metadata, links and documentation use neati.
Historical document spellings and evidence filenames were normalized; original
dates, commit IDs and screenshot pixels are not new test results.

## Typography/copy inventory and resolution

- Optional local Pretendard previously preceded the native UI face: now native
  system fonts come first, retaining Korean fallbacks without downloading fonts.
- Dense 13/18 body and 12/17 support text now use 13/20 and 12/18 leading.
  Shared page subtitles use body size and a bounded measure.
- All-caps tracked section labels now use normal tracking and sentence case;
  settings headings/actions are shorter and match the action (Copy diagnostics).
- Technical values retain tabular/monospace treatment, while ordinary status
  sentences use the body font. Cleanup's primary byte total now uses monospace.
- Observed totals read “Found in scanned locations.” Results name completed
  actions without claiming a disk-space increase. Permanent removal, recoverable
  movement, unknown estimates and measured free-space change remain distinct.
- Existing keyboard-accessible disclosures retain paths/diagnostics; no safety
  policy, native material, navigation or confirmation behavior was redesigned.

## Executed local checks

- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace --quiet`: 1,160 passed; 5 intentionally ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all --check`, `just check-architecture`, `just check-version`: passed.
- `just generate-bindings`: TypeScript and both platform goldens regenerated.
- `pnpm check`: zero errors/warnings; `pnpm test -- --run`: 424 passed, 44 files.
- `pnpm build`: passed, including generated Tailwind utility verification.
- `pnpm icons:generate` / `pnpm icons:check`: 61 assets verified.
- `bash scripts/test_install_release_app.sh`: passed using temporary app fixtures
  (fresh install, unrelated-app preservation, foreign identity refusal, rollback).
- `bash scripts/test_release_workflow.sh`: passed.
- `python3 -m unittest discover -s scripts -p 'test_cleanup*.py'`: 12 passed.
- `just build-fast`: current frontend embedded in the debug neati.app bundle.
- Packaged bundle: identifier `com.neati.desktop`, version `0.3.74`.
- Packaged `Neati --doctor`: 14/14 checks passed; no cleanup performed.
- Source and packaged `icon.icns` SHA256 both
  `6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.

## Browser evidence (mock data, not native glass)

- `before-storage.png`: pre-typography-pass 960×660 browser baseline, 0.3.73.
- `storage-light.png`: 960×660, updated cleanup hierarchy/copy.
- `storage-dark-800.png`: 800×560, scrolling list and visible action toolbar.
- `overview-dark-800.png`, `settings-dark-800.png`: minimum main-window size.
- `overview-light-1440.png`: larger desktop layout.
- `quick-light-400.png`, `quick-dark-320.png`: 400/320×740; reduced-motion
  emulation enabled for the narrow dark capture.
- `korean-wrap-fixture-800.png`: a temporary browser-only Korean subtitle stress
  fixture. Two lines at 20px leading, `word-break: keep-all`, no document-level
  horizontal overflow. This is not shipped translated copy.
- `preview-refusal-light.png`: attempted browser cleanup correctly refused; no
  actual deletion and no fabricated success. Result variants are covered by
  component rendering tests, not a claimed native cleanup run.

Dark captures explicitly set the browser document's dark class; they do not
prove persisted native appearance. Keyboard Tab reached the next navigation
button. This is not an exhaustive screen-reader or keyboard-flow audit.

## Not verified / not performed

Windows execution/packaging awaits CI. Native GUI launch, fresh permission
grants, authentication, real-data cleanup, and same-backdrop native-glass QA
were not performed. The installed app was not replaced. No settings, user
credentials or previous app were removed. No release, tag, external signing
registration, WinGet submission or Homebrew publication was performed.
