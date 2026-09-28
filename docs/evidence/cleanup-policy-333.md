# Cleanup policy and accounting batch — #333

## What changed

This is an independent behavior comparison against local RC-01 1.55.0, not a
source import. The broader 25-ecosystem review and source fingerprints are in
[the #329 evidence](cleanup-coverage-329.md). No destructive command was run
against user caches or the system Trash.

- Catalog `deletion_disposition` separates rebuild consequence from mutation
  channel. Omitted values preserve existing behavior. Only Clang module caches
  and the existing named Chrome/Brave code-cache units opt into permanent
  removal. HTTP caches, DerivedData and other Rebuild entries retain their
  existing Trash behavior. Existing process, scope, identity, structured-state,
  and symlink checks still execute.
- Owner authorization carries its provider's disposition; the registry
  rechecks it before execution. DotSlash's verified moves now contribute only
  to Trash bytes, including in preview, instead of claiming removed bytes.
- Seven additional macOS observation signatures cover Poetry download/cache
  siblings, Ruby download stores, Hex, Opam, Zig and optional Ruff/MyPy user
  caches. They contribute observed bytes only, never cleanable or selected
  bytes. They are **not seven new deletion adapters**. No virtualenv, installed
  gem, Mix archive, Opam switch or whole-home project search is authorized.
- Decimal reference display units are parsed as SI; explicit KiB/MiB remain
  binary. `du -k` remains separately interpreted as KiB. The #329 ledger was
  regenerated from identical input hashes; rounded potential is not measured
  reclaim.

## Source comparison and rejected shortcuts

Reviewed local RC-01 stages include `dev.sh` Python (768–802), Ruby/Perl
(1217–1230), Gradle (2855–2892), other languages (3880–3900), Hex/Opam
(5254–5263), and Clang (738–766). Shared `safe_remove` reaches permanent
removal after its guards. That does not describe every workflow: uninstall
has a separate Trash/permanent routing policy. No code was copied.

| Area | Decision in this batch |
| --- | --- |
| Clang, browser code | Explicit permanent deletion for existing narrow cache units; do not broaden roots or remove process guards. |
| Poetry / Ruby / Hex / Opam | Add named observation roots only. Shared downloads need an owner adapter, not a generic recursive fallback. |
| Ruff / MyPy | Optional user locations are not their normal project-local cache discovery. MyPy can use SQLite; keep observation-only. |
| Zig / Bazel | Zig global output and downloads require separate ownership; observe its default path. Do not authorize Bazel install/output bases. |
| Bun | Keep advisory. Owner reset can include a shared global virtual store; matching its command name is insufficient proof of safety. |
| npm / pnpm / uv / pip / Go / Composer / NuGet | Preserve existing fixed owner commands. No reset-for-prune substitution, filesystem fallback, or guessed prune yield. Multi-generation pnpm enumeration remains unimplemented. |
| Gradle | Whole-store observation is not missed reclaim. No new generic deletion of dependency caches, daemons or workers. |
| Cargo / rustup / Maven / Dart / Haskell | Existing Cargo archive owner support remains; source stores, toolchains, repositories and installed dependencies are not newly authorized. |

Primary owner references checked September 28, 2026:

- [Bun cache command](https://bun.com/docs/pm/cli/pm) and
  [global virtual store](https://bun.com/docs/pm/global-store): reset includes
  the shared store, not merely unused downloads.
- [Ruff configuration](https://docs.astral.sh/ruff/configuration/): cache cleanup
  is rooted in a project/directory context.
- [MyPy cache options](https://mypy.readthedocs.io/en/stable/command_line.html):
  custom cache locations and SQLite storage must be accounted for.
- [Zig compilation source](https://github.com/ziglang/zig/blob/master/src/Compilation.zig):
  global-cache resolution can be overridden; a default-path signature does not
  claim complete configured-cache discovery.

## Corrected read-only machine evidence

Prior same-day scan: Zenith **0.3.71**, macOS **27.0 (26A428)**, command-line
example process, not the installed app. RC-01 input SHA-256:
`894d0254fc8d28c182b7fb10bed7b04ecfa9f1d8b5d7d542781c3f9ad3839970`.

- RC-01: 625.2 MB reported potential, 152 rows. Three nested rows contribute
  227,453,000 rounded bytes. Top-level rounded sum: 397,723,000 bytes.
- Zenith: 2,242,281,472 observed bytes; 334,581,760 cleanable bytes;
  13,123 ms scanner time, 2,866 directories and 22,869 entries, peak 16 tasks.
- Relationships: 11 exact, 98 under a Zenith ancestor, four containing Zenith
  descendants, 39 unmatched. An ancestor match does **not** establish equivalent
  deletion permission or bytes.

These are different algorithms, rounded displays and a changing workstation,
not a controlled throughput comparison. Neither 625.2 MB nor 397.7 MB is a
verified deletion result. Full Disk Access for Zenith.app does not establish
equivalent access for a CLI launched by another host. Partial CLI coverage
cannot be used to declare the installed app's permissions broken.

### Post-change read-only scan

Zenith 0.3.72, September 28, 2026, same CLI host: 2,275,803,136 observed bytes,
367,149,056 cleanable bytes and 4,096 selected bytes. The scan remained partial
(452 Full Disk Access gaps, 20 permission-denied gaps, six I/O gaps). Scanner
time was 16,174 ms; wall time 16.75 s; 2,859 directories, 23,492 entries, peak
16 outstanding tasks. No nonzero units from the seven new observation
signatures were present in this scan. Those signatures do not establish extra
reclaim on this workstation. Running-owner and explicit provider-review policy
still separate cleanable from automatically selected bytes.

Local builds and downloads occurred between scans. The changed totals and
timings must not be attributed to a cleanup improvement or slowdown from this
batch. No deletion occurred in either live scan; installed-app coverage was
not measured by this CLI run.

## Verification protocol

`repeated_cleanup_shapes_report_verified_accounting` creates private temporary
trees, runs the real scanner/planner/executor, and asserts removed allocated
bytes equal scan accounting, Trash bytes remain zero, and an outside sentinel
survives. One warm-up and five measured runs alternate shape order:

- 4,096 files × 4 KiB;
- 16 files × 8 MiB;
- 64 directories × 32 files × 8 KiB.

Run explicitly with `cargo test -p zenith-desktop --test scan_benchmark
repeated_cleanup_shapes_report_verified_accounting -- --ignored --exact --nocapture`.
It is ignored in ordinary CI to avoid repeatedly paying for a benchmark.
The regular regression suite still exercises permanent removal, recoverable
movement, policy rejection and accounting. This is a **Zenith fixture
benchmark**, not an RC-01 end-to-end deletion speed claim or a measured disk
free-space delta. Windows runtime and native glass QA were not performed.

### Executed fixture results

September 28, 2026; Zenith 0.3.72 debug build; macOS 27.0 (26A428).
All 18 runs passed, including three warm-ups. The 15 measured runs all removed
exactly the measured allocation, reported zero Trash bytes, left zero fixture
cache files, and preserved the outside sentinel. Medians of five runs:

| Shape | Allocated / removed | Scan | Plan | Execute |
| --- | ---: | ---: | ---: | ---: |
| Small files | 16,777,216 B | 696.290 ms | 17.107 ms | 1,579.568 ms |
| Large files | 134,217,728 B | 43.957 ms | 0.566 ms | 729.909 ms |
| Nested files | 16,777,216 B | 138.762 ms | 10.314 ms | 1,171.054 ms |

Execution includes the existing end-to-end executor overhead, not just unlink
syscalls. Execution ranges were 1,571.189–1,628.783 ms (small),
710.320–891.322 ms (large), and 1,161.719–1,392.328 ms (nested).
These establish a reproducible accounting/performance baseline, **not a
before/after speedup**. Only temporary fixtures were deleted; no user data was
removed and no installed/running application was replaced.

Final local verification: 1,158 Rust tests passed (five ordinary-run ignores),
419 frontend tests passed, 12 Python tests passed; typecheck, production build,
all-target Cargo check and Clippy, architecture boundaries, generated IPC,
formatting and synchronized versions passed. `just build-fast` produced
`target/debug/bundle/macos/Zenith.app`; both bundle version fields are 0.3.72.
The packaged icon matched the source ICNS hash and its extracted image was
visually inspected. No UI layout or glass change was made; native window
visual QA and Windows runtime validation were not performed. The built app
was not installed or launched.
