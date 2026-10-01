# Windows Git operand correction for PR #399

Validation date: October 1, 2026. Source checkpoint:
`e6d7a9a5d531deb16566c2ce02b4865848a12ee1`. Host: macOS 27.0.1
(26A434), arm64. This follow-up retains PR #399's **0.3.94 → 0.3.95**
transition; no additional patch bump was run. Fixes #390; refs #391's
observation slice, with framework cleanup still unavailable.

## Failure and correction

The [first follow-up Windows run](https://github.com/jaeyoung0509/neati/actions/runs/36839478151/job/110294855143)
passed lint but failed the positive generated-artifact plan in
`fresh_authored_content_at_planning_and_final_boundary_never_reaches_trash`.
The ownership probe could not verify metadata, so cleanup stayed unavailable.
The job stopped before platform-crate tests ran; Windows packaging was skipped.

Read-only review identified a Git boundary mismatch: canonical native
`\\?\` paths reached Git's argv and environment unchanged. Git for Windows
[validates file operands before its long-path conversion](https://github.com/git-for-windows/git/blob/main/compat/mingw.c#L3861-L3978)
and its NTFS guard refuses `?`. This source inference motivated the correction;
the native Windows CI fixtures must confirm the final behavior.

Only Git-facing path operands are adapted with the existing parameterized path
algebra: `--git-dir`, `--work-tree`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE` and the
private configuration `--file`. Drive roots keep their root separator. Invalid
Unicode, drive-relative paths, parent traversal, short aliases, ADS, trailing
dot/space and reserved devices remain uncertain. POSIX paths retain their
original `OsStr` values. Native route paths, component identities, no-link
checks and before/after bindings remain unchanged. NTFS protection is retained.

The portable regression checks drive, UNC, Korean/spaced and drive-root
operands, every argv/environment/configuration path, unchanged native routes,
and refusal of ambiguous paths. The desktop fixture now supplies typed fresh
ownership evidence if its initial plan fails, while preserving every positive
and protected-move assertion.

## Local evidence

[local-checks.json](local-checks.json) records the final source verification and
full-log hashes. Format, workspace check, all-targets Clippy with denied
warnings, **1,335 Rust tests**, architecture, binding generation/drift,
version, icon and final bundle checks passed. Six default ignored Rust tests
remain unchanged. Frontend results (**485 tests**, zero typecheck errors or
warnings and production build) are retained from `47ce537`: subsequent source
changes are confined to Rust ownership code/tests. The final `.app` rebuild
also rebuilt the frontend.

Both bundle versions are **0.3.95** and its icon matches the source digest.
The packaged executable passed [all 14 doctor checks](doctor.json) and embeds
[all 20 current production assets byte-for-byte](embedded-assets.json).
Its HTML routes to `index-Ej6E9PNk.js`.

No UI or asset changed: the earlier 43 browser checks and seven screenshots
remain applicable. Source review ran no builds or tests; the primary agent
performed the recorded checks. The [PR description](https://github.com/jaeyoung0509/neati/pull/399)
reports the latest native Windows Rust and packaging CI result separately.
Interactive Windows/Linux use and native tray/glass behavior remain unverified.
The local bundle was not installed or launched as a GUI, and no real-user
cleanup ran. Full logs and complete Git bundles remain outside temporary storage.

The Git operand CI run subsequently passed the desktop planning and native
file-ID fixtures, but failed three platform fixtures and skipped Windows
packaging. The [directory identity follow-up](../directory-entities/README.md)
records their correction and the newer local verification checkpoint.
