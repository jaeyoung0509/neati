# Windows CI follow-up for PR #399

Validation date: October 1, 2026. Source checkpoint:
`633337cfb0421c726fa0c0e41f53744b4a799d19`. Host: macOS 27.0.1
(26A434), arm64. This is a follow-up to
[PR #399](https://github.com/jaeyoung0509/neati/pull/399), which fixes
[#390](https://github.com/jaeyoung0509/neati/issues/390) and references the
observation slice of [#391](https://github.com/jaeyoung0509/neati/issues/391).
The PR's **0.3.94 → 0.3.95** transition is unchanged; no second bump was run.

## Corrections and regression coverage

The [initial Windows Rust job](https://github.com/jaeyoung0509/neati/actions/runs/36828901135/job/110260736525)
failed at `clippy::needless_return` in the Windows-only file-identity block.
The block now uses its result as the tail expression, with the same behavior
and without relaxing the warnings-as-errors gate.

Independent read-only review also found that valid linked worktrees could be
refused on Windows: the canonical `gitdir` route and lexical `commondir` route
had different path spellings. Rust's Windows canonicalization uses extended
length syntax ([standard-library documentation](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)).
The recorded common route is now bound before canonicalization, compared in
canonical form, and bound again afterward. Original and canonical component
identities remain part of the post-probe verification; unrelated common roots,
links and forged backlinks remain refused.

The linked-worktree fixture verifies relative, absolute lexical and native
canonical metadata routes. It also checks tracked-content protection, an
unrelated common root and a forged backlink. Its Windows-native branch must
execute on Windows CI; a macOS result does not establish that platform's
runtime behavior.

## Local verification

[local-checks.json](local-checks.json) records command results and log hashes.
Full logs remain in the durable task recovery directory outside temporary
storage. All Rust checks and the bundle ran at the source checkpoint above.
The frontend checks ran at `47ce537`; the only subsequent source change is
`crates/neati-platform/src/artifact_ownership.rs`, so those successful frontend
results are retained. The final bundle rebuild also rebuilt the frontend.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --offline --locked --workspace` | Passed |
| All-targets Clippy with `-D warnings` | Passed |
| `cargo test --offline --locked --workspace` | 1,334 passed, 0 failed, 6 default ignored |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 485 passed |
| `pnpm build` | Passed |
| Architecture, binding export and drift checks | Passed |
| `just check-version` | All synchronized outputs are 0.3.95 |
| `pnpm icons:check` | 61 assets/registry entries verified |
| `just build-fast` | Debug arm64 `.app` built; both bundle versions are 0.3.95 |
| Packaged `Neati --doctor --json` | [14 passed, 0 failed](doctor.json) |
| Packaged icon | Source-identical SHA-256 |
| Embedded production assets | [All 20 match byte-for-byte](embedded-assets.json) |

No UI or visible asset changed in this follow-up. The earlier 43 browser checks
and seven screenshots remain applicable. The rebuilt executable embeds the
same current frontend entry, `index-Ej6E9PNk.js`.

## CI and platform limits

Local results are separate from the current PR's check status. The initial
run failed before Windows runtime tests executed, and its Windows package job
was skipped. The PR description reports the latest follow-up CI run and
packaging result. Windows file-ID/time-restoration and linked-worktree fixtures
need that native runner; interactive Windows/Linux use and native tray/glass
behavior remain unverified. This local bundle was not installed or launched
as a GUI. No real-user cleanup was performed.
