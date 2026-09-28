# Neati B rebrand verification

Date: September 28, 2026. Host: macOS 27.0 (26A428), Apple Silicon.
Version: 0.3.73, branch `feature/334-neati-rebrand`, based on `fc3c0ce`.
Evidence belongs to this PR's implementation commit, not the earlier sketches.

## Executed

- `cargo check --workspace --all-targets`
- `cargo test --workspace --quiet` (1,160 passed, 5 ignored)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`, `just check-architecture`, `just check-version`
- `just generate-bindings` (native bindings and both platform golden files)
- `pnpm check` (zero errors/warnings), `pnpm test -- --run` (423 passed)
- `pnpm icons:generate`, `pnpm icons:check` (61 generated assets/registry outputs)
- `pnpm build`, `just build-fast`
- `bash scripts/test_install_release_app.sh` (temporary fixtures only)
- Packaged `Neati --doctor`: 14/14 passed.
- Bundle plist: Neati, 0.3.73, `com.zenith.desktop`, executable Neati.
- Source and packaged icon.icns SHA256 both:
  `6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.

## Visual evidence and limits

- `main-light-browser.png`, `main-dark-browser.png`: 960×660 preview.
- `main-800-browser.png`: 800×560 preview.
- `quick-light-browser.png`: 400×740 preview.
- `quick-dark-320-browser.png`: 320×740 preview, reduced-motion emulation.
- `main-native.png`: actual packaged app, 960×660 logical window at 2× scale.
  Main sidebar wordmark, icon, window title, footer and Settings/About branding
  were inspected. Existing provider selection/navigation order were visible.
- Inspected generated 128px package icon and the monochrome template source;
  geometry is covered by the rebrand regression test. The retired Z master was
  removed; Git history retains it.

Browser evidence is not native glass QA. No native material/tint/opacity changes
were made. Native Quick Panel/tray/Finder-Dock appearance was not fully captured:
menu-extra automation timed out. No claim of a same-backdrop glass comparison or
full native dark-theme matrix is made. Windows packaging/legacy-registration
refusal is covered by the updated Windows CI smoke script but was not executed
on this macOS host. CI status is reported on the PR, separately from local checks.

## Installation and permissions

With explicit user approval, the existing installed Zenith 0.3.70 was normally
quit, the build-tree Neati.app was launched, then normally quit and the original
installed Zenith was reopened. `/Applications/Zenith.app` was never replaced,
moved or removed. No new app was installed, no cache deletion was invoked, no
permission grants were changed and no secret values were inspected.

Startup's ordinary read-only scan reported protected locations as unreadable
(456 locations in the final main-window evidence). TCC/Full Disk Access does
not automatically transfer merely because a bundle identifier is retained.
This is not evidence of complete scan coverage or successful permission migration.

Issue #334 retains its public-name clearance, distribution/signing and remaining
native/Windows release gates. Homebrew/tap publication and CLI #335 are not
implemented by this PR.
