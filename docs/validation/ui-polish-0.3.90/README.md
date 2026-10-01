# UI polish #378 — local verification

Version 0.3.89 → 0.3.90, starting from
`develop@ddcfe3937e5380448dcde1d53036ff209352d39f`.
Executed October 1, 2026 on macOS 27.0.1, build 26A434 (Apple Silicon).
This is a validation record for the initial whole-app implementation, not a
claim of completed native or visual acceptance. Issue #378 remains open.

## Executed checks

| Check | Result |
| --- | --- |
| `pnpm check` | Passed; 0 errors, 0 warnings |
| `pnpm test -- --run` | Passed; 476 tests in 50 files |
| `pnpm build` | Passed |
| `cargo check --workspace --locked` | Passed; cached dependencies, offline |
| `cargo test --workspace --locked --no-fail-fast` | Failed under session restrictions; details below |
| `just check-architecture` | Passed |
| `just check-version` | Passed; all six synchronized outputs at 0.3.90 |
| `just build-fast` | Passed; debug macOS application bundle rebuilt |
| Local source review | Stop ID gating, motion cleanup and fixture failure/recovery reviewed |
| CI | Not run; no remote PR has been published from this worktree |

Rust's existing OpenRouter callback test cannot bind a loopback listener;
the native doctor test cannot create its log directory; and the development
port integration helper cannot bind its temporary loopback socket. Tests and
assertions were not changed to hide these restrictions. A separate complete
workspace run with these three explicitly named filters passed. The unfiltered
suite is still recorded as failed, not successful. The final serialized
unfiltered run completed 1,268 passing tests, three failures and five existing
ignored tests; its doc tests completed successfully.

```sh
cargo test --workspace --locked -- \
  --skip ai_providers::openrouter::tests::a_silent_connection_times_out_and_a_later_callback_is_still_accepted \
  --skip diagnostics::doctor::tests::run_cli_exit_codes_follow_the_checks \
  --skip release_integration::test_controlled_integration_ephemeral_loopback_release
```

The first final unfiltered run overlapped packaging in the same target directory
and also failed rustdoc dependency lookup. Final verification is serialized
after packaging; do not run a Tauri build and workspace tests concurrently in
the same target directory. Other issue worktrees use their own cloned targets.

The .app reports `CFBundleShortVersionString` and `CFBundleVersion` 0.3.90.
Its icon SHA-256 matches `src-tauri/icons/icon.icns`:
`6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.
Current Dashboard, QuickPanel and shared frontend asset keys are embedded in
the packaged executable. The installed/running application was not replaced
or launched. No signing, release or tag was performed.

## Reproducible browser evidence

The mounted browser fixture uses production Svelte components and deterministic
synthetic browser mocks. It does not inspect directories or invoke native IPC.
Generate a self-contained HTML file that can be opened offline:

```sh
node scripts/build_ui_polish_validation.mjs /private/tmp/neati-ui-polish-validation
```

Open the resulting `ui-polish.html`. Use its `window.uiPolishValidation` driver
to select scan states (`initial`, `preparing`, `scanning`, `stopped`, `empty`,
`unavailable`, `privacy`, `partial`, `retained`, `cleaning`, `refreshing`,
`failed`, `ready`), switch light/dark themes, change actual values, or populate
long translated names. Failed/accepted inventories go through the real public
scan-store workflow so the fixture respects selection invalidation and recovery.
Preparing has no scan ID; the Stop control becomes usable only after Started.

The existing static Cleanup generator also ran successfully, producing 16
labeled light/dark SSR HTML files for initial, empty, unavailable, privacy,
available, partial, retained and mixed inventories:

```sh
pnpm build
node scripts/render_cleanup_validation.mjs /private/tmp/neati-378-static
```

Generating HTML is not a screenshot or a mounted-layout check. This session's
local server bind was refused (`EPERM`), the in-app browser was unavailable,
and Computer Use did not approve Chrome access. No alternate permission path
was used. Prior-version prototype captures do not prove this version's layout.

## Required before leaving draft

- Capture current light/dark Overview, Cleanup progress/results, Performance,
  Memory, AI Activity, Settings and Quick Panel at their supported minimum and
  representative sizes. Check long names and keyboard focus.
- Exercise preparation, cancellation, post-cleanup checking and failure/recovery
  without stale actionable rows. Check reduced motion/transparency and hidden
  surfaces; actual readings must remain immediately visible.
- Compare the packaged native main/quick windows with the baseline glass and
  verify durable Quick Panel bounds across slow/late metrics and activation.
- Run the unfiltered suite in an environment that permits its existing native
  fixtures and record CI separately. Windows native runtime remains unverified
  and belongs to #380.

These items have not been executed here. No native permission transition,
real-user cleanup, app installation or Windows desktop test is claimed.
