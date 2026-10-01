# Directory identity follow-up for PR #399

Validation date: October 1, 2026. Source checkpoint:
`26ef04d4db9d0f22fa56163388d79331cd730c8b`. Host: macOS 27.0.1
(26A434), arm64. This follow-up retains the same PR's **0.3.94 → 0.3.95**
transition; no additional patch bump was run. Fixes #390; refs #391's
observation slice. Framework cleanup remains unavailable.

## Native Windows failures and correction

The [Git operand CI run](https://github.com/jaeyoung0509/neati/actions/runs/36841537829)
passed both desktop generated-artifact planning fixtures and the Windows
full-file-ID regression. Three platform fixtures still failed: the test's own
`git worktree add` received a verbatim native path, and malformed-index and
missing-object fixtures observed `ChangedDuringProbe` before their expected
metadata reasons. Windows packaging was skipped; macOS Rust and packaging passed.

Source review found that identity-only ancestor and repository-absence bindings
called the full content-stamp helper. On Windows that helper compares directory
size and modification time across two handle observations before obtaining its
native ID. Independent sibling writes under shared temporary ancestors can
therefore revoke an identity-only binding. The old log does not identify the
rejecting path; this explains a concrete contract mismatch, and native CI must
confirm that the failure is resolved.

Those bindings now store a distinct native entity. Windows obtains the complete
volume/file ID from a fresh no-follow handle, with initial and opened directory
and no-reparse checks. Unavailable or zero IDs remain uncertain. POSIX retains
device/inode identity. This changes only sites that already compared entity
identity. Full content stamps remain on the artifact tree, repository marker,
index, HEAD and configuration; opened-read and post-read checks are unchanged.
Repository absence still re-enumerates the `.git` namespace. The fixture's
worktree-creation operand uses the existing native lexical path normalization.

Two regressions run on every platform: a forced ancestor timestamp change and
sibling write retain the original entity while a new `.git` entry revokes the
absence binding; a same-size write to the same metadata file revokes its full
content stamp. The native Windows replacement fixture additionally requires
the entity-only budget to reject a different file ID even after creation/write
timestamps are restored. Existing malformed/missing metadata fixtures retain
their exact expected reasons. No retry, sleep, ignored test or permissive
uncertainty assertion was introduced.

## Local verification and limits

[local-checks.json](local-checks.json) records successful format, workspace
check, all-targets Clippy with denied warnings, **1,337 Rust tests**,
architecture, regenerated bindings with no drift, version/icon checks and the
final `just build-fast` bundle. Six existing default ignored tests are unchanged.
The focused ownership suite also passed all 24 tests. Frontend typecheck
(zero errors/warnings), **485 frontend tests** and production build are retained
from `47ce537`, with source parity explicitly checked. Subsequent changes are
Rust-only; the final bundle rebuilt the frontend again.

Both bundle versions are **0.3.95** and the packaged icon matches its source.
The executable passed [all 14 doctor checks](doctor.json) and embeds
[all 20 current production assets byte-for-byte](embedded-assets.json).
Its HTML routes to `index-Ej6E9PNk.js`.

The earlier 43 browser checks and seven screenshots remain applicable; no UI
or asset changed. A second agent reviewed the source without running builds or
tests; the primary agent performed the recorded verification. The
[PR description](https://github.com/jaeyoung0509/neati/pull/399) reports the latest
native Windows Rust and packaging CI results separately. Interactive native
Windows/Linux, tray/glass behavior and GUI cleanup remain unverified. No
real-user cleanup, installation or GUI launch ran. Full logs and complete Git
recovery bundles remain outside temporary storage.

The owner subsequently authorized the ordered merge of #398 and #399.
The [0.3.96 integration proof](../integration-0.3.96/README.md) records the
new target/version synchronization and fresh local verification.
