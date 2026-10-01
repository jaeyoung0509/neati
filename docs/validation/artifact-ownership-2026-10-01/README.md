# Developer artifact ownership and framework observations

Validation date: October 1, 2026. Version: **0.3.94 → 0.3.95**, using one
`just bump-patch` after scope settled. Host: macOS 27.0.1 (26A434), arm64.
The final source checkpoint is
`e333f75d55fef8ddcc8dd26fc621ad5517fe05c1`; the initial browser interaction
checkpoint was `e0ff207`, before binding export and test-only Clippy fixes.
The shipped UI remained frozen between those checkpoints.

The latest Rust-only source checkpoint is
`e6d7a9a5d531deb16566c2ce02b4865848a12ee1`. The
[Windows Git operand correction](git-operands/README.md) records its final
1,335-test local verification and rebuilt bundle. The earlier
[Windows CI follow-up](ci-followup/README.md) records the lint and canonical
linked-worktree corrections. Version **0.3.95** is retained for this same-PR
follow-up; the initial evidence below records its earlier checkpoints.

This batch implements [#390](https://github.com/jaeyoung0509/neati/issues/390)
and the observation slice of
[#391](https://github.com/jaeyoung0509/neati/issues/391). Authored-content
protection is independent of byte measurement and is re-derived at discovery,
planning and the final whole-unit Trash boundary. Git-tracked paths, nested
repository markers and deployment keypair filenames revoke cleanup authority;
key contents are never opened. Unknown, incomplete or changed ownership
evidence also revokes authority while preserving observed bytes.

Exact default `.svelte-kit` and `.next` units require bounded direct dependency
evidence and remain unselected observations. Their build/dev owner-use,
deployment/offline ownership, removal and fresh mutation contracts are still
unverified. Link #391 as **Refs**, rather than closing the entire issue. Custom
configuration is not evaluated, and unverified wrappers retain nested-package
discovery. The existing 16-workspace and 512-candidate budgets remain in place.

## Local checks

All commands completed successfully against the .95 source. Full command logs
are retained in the durable recovery verification directory; the portable
results are in [local-checks.json](local-checks.json).

| Check | Result |
| --- | --- |
| `cargo check --workspace` | All workspace crates compiled |
| `cargo test --workspace` | 1,334 passed, 0 failed; 6 default ignored |
| `just lint-rust` | Format and all-targets Clippy with `-D warnings` passed |
| `just check-architecture` | No forbidden edges; [output](architecture.txt) |
| `just generate-bindings` | Three exports passed; repeat generation byte-identical |
| Scan baseline export and diff | Passed; no generated baseline drift |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 485 passed in 50 files |
| `pnpm build` | Production build and Tailwind verification passed |
| Cleanup privacy tests | 12 passed |
| Release/installer regressions | Temporary fixture recipes and rollback passed |
| `just check-version` | [All synchronized outputs are .95](check-version.txt) |
| `pnpm icons:check` | [61 assets/registry entries verified](icons-check.txt) |
| `just build-fast` | Debug arm64 `.app` bundle built |
| Packaged `Neati --doctor` | [14 passed, 0 failed](doctor.txt) |
| Embedded assets | All 20 production assets match byte-for-byte |

The three binding exports and ignored scan-baseline generator were executed
separately. The two ignored local benchmark loops were not run. Remote feature
CI has not run at this local handoff; its result must be reported separately.

The ownership regressions cover live split-index substitution without touching
the real shared index, hostile Git environment/config/hooks/fsmonitor, original
metadata corruption, linked worktree routing, every ancestor index, Unicode and
Windows path ambiguity, repository/descendant replacement, incomplete probes,
and name-only protected content. Recording-Trash fixtures assert zero moves for
blocked targets and a move for a verified generated unit. Framework observations
cannot gain planning or final-move authority through forged presentation states.

## Browser evidence

[browser-checks.json](browser-checks.json) records **43/43 assertions**, zero
page errors and seven screenshots from owned Chromium browser mocks. These
exercise explicit manual selection, disabled framework/protected selections,
partial-byte selection, bulk eligibility, keyboard focus/review/cancel,
framework evidence disclosure, reduced motion, and small light/dark layouts.
The production preview loads `index-Ej6E9PNk.js` from the built .95 dist.
These screenshots are browser evidence, not native Tauri-window evidence.

- [800×560 light](screenshots/frameworks-light-800x560.png)
- [800×560 dark](screenshots/frameworks-dark-800x560.png)
- [960×660 light](screenshots/frameworks-light-960x660.png)
- [960×660 dark](screenshots/frameworks-dark-960x660.png)
- [1280×900 manual review](screenshots/review-light-1280x900.png)
- [1280×900 production preview](screenshots/frameworks-light-production-1280x900.png)
- [1280×900 keyboard evidence disclosure](screenshots/framework-evidence-light-production-1280x900.png)

## Native bundle and limits

[native-bundle.json](native-bundle.json) records the executable digest, .95
bundle versions and packaged icon digest. The 137,274-byte icon is identical to
`src-tauri/icons/icon.icns`; branding was unchanged. The debug executable has a
[linker ad hoc signature](bundle-signature.txt), with no TeamIdentifier or sealed
resources. No release signing, installation or running-app replacement occurred.

[embedded-assets.json](embedded-assets.json) records byte comparisons for every
current production asset. The verification Brotli-decompressed generated Tauri
assets, compared them with `dist/assets`, then found those exact compressed
bytes in the packaged executable. Embedded HTML references the same current
entry script as browser preview.

Independent read-only source review found no remaining concrete blockers. The
reviewer ran no builds or tests. The Windows native file-ID/time-restoration
fixture is present but remains unexecuted on this macOS host; Windows/Linux
runtime and packaging behavior and the native GUI are unverified locally.
Unsupported file identities, split-index/non-SHA-1 layouts, ambiguous Unicode
equivalence and unverified repository metadata remain fail-closed. Git sees
private checksum-verified index/configuration copies, with live source metadata
rebound before and after the bounded child probe.

No real cleanup action was performed. The original dirty checkout remained
untouched, and all cleanup/race fixtures used temporary project trees.
