# Empty XDG cache-root validation — 0.3.95

Issue [#394](https://github.com/jaeyoung0509/neati/issues/394), related to [#381](https://github.com/jaeyoung0509/neati/issues/381). This PR changes the platform root resolver and regression fixtures; it has no interface or asset changes relative to its target.

The final source starts from develop `241459e716d5514818bbf69b112b172864c32321` (0.3.94) and uses `just bump-patch` for **0.3.94 → 0.3.95**. Earlier temporary worktree and build outputs were deleted by the owner. Source was recovered from the recorded edits into a persistent worktree and all checks below ran again on that recovered source. Old pre-deletion checks are not counted here.

On October 1, 2026, macOS 27.0.1 (26A434), arm64:

| Check | Result |
| --- | --- |
| `pnpm check` | Passed, no errors or warnings |
| `pnpm test -- --run` | 483 tests passed |
| `pnpm build` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --offline --locked --workspace` | Passed |
| `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --offline --locked --workspace --no-fail-fast` | 1,308 passed, zero failed or filtered, six existing ignored |
| `just check-architecture` | Passed |
| `just generate-bindings` and binding drift | Passed; generated outputs unchanged |
| `just check-version` and `pnpm icons:check` | Passed |
| `just build-fast` | Passed; debug application bundle built |
| Bundle and `--doctor --json` inspection | Both versions 0.3.95, source-identical packaged icon, all 13 current JS/CSS asset names embedded, 14 doctor checks passed |
| `git diff --check` | Passed |

[local-checks.json](local-checks.json) records commands, exit codes, timings, log hashes, source/base commits and bundle asset hashes. [doctor.json](doctor.json) contains the de-identified diagnostic result. Verification used three Cargo build jobs and disabled incremental output to limit parallel-worktree disk and memory use.

The six new regression tests exercise unset/empty equivalence for POSIX and Windows path flavors, nonempty valid and refused overrides, missing home, unrelated variable semantics, nonzero advisory namespaces and exact JavaScript payloads. Expanded existing fixtures cover forged advisory selections and linked roots/ancestors with an empty variable. The JavaScript test refuses a fresh plan after Node starts; execution guards are unchanged. All cache payloads are temporary fixtures, and no real application cache was removed.

Windows path-flavor assertions ran on this macOS host. Native Windows and Linux execution was not performed; Linux desktop capabilities remain unavailable. A screenshot is unnecessary because this PR changes no interface or visual assets relative to develop. CI status must be read separately from these local results. The debug bundle was not installed or launched as a GUI; only its diagnostic command ran. The installed neati application was not replaced by this work.
