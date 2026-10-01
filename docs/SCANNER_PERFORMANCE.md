# Scanner performance and validation

What the cleanup scan is allowed to do, what it reports about what it did, and
how a regression is detected. The contract here is enforced by tests in
`src-tauri/src/scanner` and by the benchmark in
`src-tauri/tests/scan_benchmark.rs`; the platform-specific validation lives in
[WINDOWS_VALIDATION.md](WINDOWS_VALIDATION.md) and [SAFETY.md](SAFETY.md).

## Traversal bounds

Every walk runs under one stated set of limits
(`scanner/observation.rs`, `ScanLimits::default`):

| Bound | Value | What it bounds |
|---|---|---|
| `max_depth` | 32 | Directory levels below a root. A deeper entry is counted and reported as the reason a measurement is incomplete — never silently skipped. |
| `max_concurrent_directory_reads` | 16 | Directory tasks a walk may keep outstanding at once. |

The work executes on one process-wide, explicitly bounded Rayon pool
(`execution_budget::shared_scan_pool`, at most four workers, capped by the
machine's performance cores on macOS). The limits above bound the *queue*, not
the threads: past `max_concurrent_directory_reads` the walk descends inline on
the worker that is already running, so a tree of a million directories is work
for four threads rather than a queue of a million tasks. Both numbers are
stated once and threaded through `WalkContext`, so a walk cannot pick up the
bound in one path and miss it in another.

The benchmark asserts the bound as a measurement: for every fixture the run
reports `peak_outstanding_directory_tasks <= max_concurrent_directory_reads`,
and the wide fixture reaches the bound rather than trivially satisfying it.
The inline and pooled schedulers are also required to report the same traversal
facts; scheduling changes execution, not the meaning of `visited_entries`.

### Determinism

Two scans of one tree state the same items in the same order, with the same
bytes, the same skipped counts, and the same incomplete reason. Reasons are
chosen by the walk's own order — shallowest failure first, then path, then
message — rather than by whichever worker finished first
(`scanner/size.rs`, `FailureRecord`), so a parallel walk and a sequential walk
of the same tree report identical results.

### Metadata reuse

The size and age walkers classify links from the same `symlink_metadata`
observation used for size/type accounting. They do not perform a second stat
just to ask whether an ordinary entry is a link. Windows reparse points still
require a handle-level tag query; a failed query reports an incomplete scan,
never permission to descend.

The aged tree walker already gathers size, file count and newest modification
time in one recursive pass. It prepares the environment's POSIX comparison
keys and the signature's expanded exclusions once per enumerated namespace
root, then reuses only that immutable vocabulary for its descendants. An
independent tree measurement prepares its own vocabulary. This avoids building
the protected-directory environment and normalizing every protected name for
each file; candidate paths still use the same blacklist and path-algebra rules.
The key map is bounded by the policy's names, not by the size of the tree.

The ordinary size walker now prepares that same blacklist and exclusion
vocabulary once per measurement, and shares it across its serial or bounded
parallel descendants. Separate measurements prepare their own policy; the
regression test changes both file metadata and the environment between walks.

No candidate metadata, use verdict, filesystem identity, or authorization is
cached. Every recursive entry still checks cancellation and reads fresh
`symlink_metadata`; planning and execution retain their fresh guards. Windows
8.3 alias resolution is filesystem-dependent and remains uncached, including
the resolution of aliased environment facts. macOS case and Unicode-equivalent
protection, Linux byte-exact matching, exclusions, depth limits and protected
entries retain their existing semantics.

Overlap resolution uses a disposable `ScanRelationships` cache for filesystem
identity and actual directory-entry spelling. Its lifetime is one resolution
pass, including one fresh pass when scan slices are merged. Missing identities
are cached for that pass too. The shared relationship algorithm still keeps
different hardlink entries separate, resolves case aliases using real identity,
and falls back to case-sensitive text when identity is unavailable.

This is an observation optimization, not a cleanup authorization cache. The
planner uses the uncached probe, and execution's identity, scope, symlink, and
structured-state checks are unchanged. The regression suite asserts one probe
per distinct path during an all-pairs overlap fixture, unchanged classifications,
and fresh observations after a file is replaced.

## What a scan reports

`ScanResult.metrics` (`ScanMetrics`) carries the run's own measurements:

| Field | Repeats across scans? | Meaning |
|---|---|---|
| `visited_entries` | yes | Files and directories the traversal looked at, including the ones it refused. |
| `directories_read` | yes | Directories whose contents were read. |
| `duration_ms` | no | Wall-clock duration of this run. |
| `peak_outstanding_directory_tasks` | no | The highest number of directory tasks this run kept outstanding. |

`ScanResult.spans` separately records `scan.overlap_resolution` for a scan's
cross-category overlap pass and `scan.overlap_merge` for the fresh pass that
combines completed slices. Merged duration includes the slice durations plus
that merge pass; it is not elapsed UI time including idle periods between scans.
These spans complement the signature/provider timings without changing the
meaning of traversal counters.

The bounded phase vocabulary is:

| Scope | Aggregate phases |
|---|---|
| Scan | `scan.process_snapshot`, `scan.pool_startup`, existing overlap resolution/merge and physical audit |
| Filesystem signature | `.root_expansion`, `.use_checks`, `.owner_state_projection` |
| Enumerated aged root | `.aged.policy_preparation`, `.aged.tree_measurement`, `.aged.enumeration_and_classification` |
| Inside aged measurement | `.aged.metadata_and_links`, `.aged.age_evaluation`, `.aged.size_accounting`, `.aged.traversal_and_policy` |
| Inside enumeration | `.aged.child_classification` |
| Cache provider | `.discovery` (resolution/command startup/output), `.measurement` |
| Owner provider | enclosing catalog ID and `.item_projection`; Homebrew additionally splits `.process_use_check`, `.executable_discovery`, `.prefix_query`, `.version_query`, `.dry_run_preview`, `.preview_parse` |

Tree traversal is the measurement's remainder after metadata/link, age and size
work; it includes path policy, enumeration, recursion and folding. Enumeration
excludes policy preparation and descendant measurement. All phases aggregate
across a signature's roots/children: no per-file span or progress event is added.
IDs contain only catalog IDs and phase names. Millisecond values are truncated,
so `0` can mean work below one millisecond. Enclosing and nested spans must not be
summed. A catalog ID can have both a filesystem pass and an owner pass.

`ScanResult.cancelled` states whether the run stopped because it was cancelled;
a cancelled scan is `quality: partial` with a stated reason, and the flag is
what lets the interface say *which* kind of incompleteness it is.

### Progress

`ScanEvent::RootStarted { category, signature_id, name, root }` is emitted once
per root, before the walk reads it. A scan spends most of its time inside one
root, so naming the root is what lets the interface show where a long scan is;
per-category item counts arrive with `CategoryFinished`.

### Cancellation

`cancel_scan(scan_id)` sets the signal the scan registered under the id its
`Started` event carried. The probe is consulted at every category, signature,
directory, and entry boundary, so a cancel stops the walk at the next boundary
and the partial result says so. The registry that owns those signals has a TTL for abandoned bookkeeping and a
hard entry cap. A still-running scan keeps another reference to its signal, so
age alone cannot make its Stop control expire; entries are removed on success,
on cancellation, and on error (`services::CancellationRegistry`).

Cancellation latency — the time from the request to the scan returning — is
measured by the benchmark (`cancellation_latency_is_measured_from_the_first_candidate`),
which also asserts the work actually stopped: the cancelled run reads fewer
directories than the fixture contains and never produces the later candidates.

Owner discovery checks Stop before and after each provider, and Homebrew's
read-only child commands receive the signal. Generic cache use checks also
receive it: an interrupted open-file read is unknown, and later unit probes do
not start. Observed rows remain present without cleanup eligibility when their
required check did not complete. Planning/execution retain fresh, uncancelled
checks. Disposable command fixtures assert return within the unchanged one-second
Stop ceiling, including process-group teardown; this is backend evidence, not
native-window input latency.

## Benchmark and regression guard

```bash
# Run the harness and print this machine's metrics table.
cargo test -p neati-desktop --test scan_benchmark -- --nocapture

# Regenerate the committed baseline after adding or changing a fixture.
cargo test -p neati-desktop --test scan_benchmark -- --ignored --exact export_scan_baseline
```

`src-tauri/tests/fixtures/scan-baseline.json` holds the deterministic facts per
fixture — visited entries, directories read, candidate count, skipped entries,
allocated bytes — plus the stated bounds and a generous duration ceiling. CI
regenerates the file on both platform jobs and fails on a diff, exactly like the
TypeScript bindings; the same jobs print the metrics table so a real-machine
baseline can be read out of a log.

A regression is therefore caught two ways: a **count** that moves (exact
comparison — a scanner that walks more or less than it did is a defect, not a
slow run) and a **duration** that exceeds its ceiling (a bound with at least an
order of magnitude of headroom, because CI runners are shared and slower than a
developer machine). Timings are deliberately not compared byte-for-byte.

Fixtures currently covered: `wide` (200 directories), `deep` (past the depth
limit), `mixed_size` (3 B to 2 MiB), `mixed_age` (stale and fresh siblings),
`aged_observation` (2,048 files in two advisory namespaces, one stale and one
fresh), `overlapping_roots` (one location, two rules), plus unix-only `inaccessible`
and `symlink` fixtures that are asserted by their own tests.

The advisory fixture asserts 2,307 visited entries, 259 directory reads, two
inventory units, 8 MiB of logical data and zero cleanup authority. Its cancellation
case trips during the first unit and verifies that later descendants and the
second unit remain unwalked. `repeated_aged_observation_reports_scan_cost` is an
ignored, opt-in read-only fixture benchmark; it measures a warm-up and five runs
without reading the user's temporary folders or constructing a cleanup plan:

```sh
cargo test -p neati-desktop --test scan_benchmark \
  repeated_aged_observation_reports_scan_cost -- --ignored --exact --nocapture
```

### Controlled aged-namespace comparison, 2026-10-01

The [0.3.90 validation record](validation/2026-10-01-aged-observation-0.3.90.json)
compares the base `ddcfe393` production code at 0.3.89 against `86b3b2f` at
0.3.90 using the same disposable fixture and debug test profile. The host's
previously recorded hardware is a MacBook Air M1 with 16 GiB; this run read
macOS 27.0.1 (26A434) from the system version file. Other agents' builds and
tests were paused, while ordinary OS/application activity continued.

Six processes ran in before/after/after/before/before/after order. Each process
created its own fixture, warmed it once and measured five repeated scans, giving
15 measured samples per version. Every scan asserted the same 2,307 visits,
259 directory reads, two units, 8 MiB of logical data, no skipped entries and
zero cleanup authority.

| Fixture measurement | 0.3.89 | 0.3.90 |
|---|---|---|
| Median scan wall time | 435.398 ms | 185.242 ms |
| Measured scan range | 432.786–443.114 ms | 184.802–185.956 ms |
| Median reported scan RSS growth | 16 KiB | 64 KiB |
| Median whole-process CPU time | 2.942291 s | 1.442530 s |

Median wall time fell 57.45% for this synthetic aged-namespace fixture. This
does not establish a speedup for the live 36-second scan or its 18-second
temporary-folder category, GUI/IPC progress, provider startup or Windows.
Whole-process CPU includes fixture construction, test startup and all six scans;
RSS is approximate and does not show a memory improvement here. The aged walk
remains sequential and the shared pool/queue bounds are unchanged.

The record also lists the local check results, the final debug bundle's version
and icon hash, and the three unchanged Rust tests blocked by managed-environment
loopback/log-directory restrictions. The unfiltered suite failed on those
tests; a separate run with exactly those three named exclusions passed. Neither
result substitutes for CI or native Windows validation, which have not run.

### Controlled ordinary-walker comparison, 2026-10-01

The `repeated_plain_observation_reports_scan_cost` ignored benchmark compares
the ordinary walker from `63a8a3e` (0.3.96 behavior) with this change in the same
debug harness. Both binaries carry the reserved 0.3.97 label. Each process
warms up once and measures five scans on MacBook Air M1, 16 GiB, macOS 27.0.1
(26A434). Every scan asserts 2,305 visits, 257 directory reads, one advisory
unit, 8 MiB logical data, zero cleanable bytes, no skipped entries and eight
events. Both runs reached the unchanged 16-task bound; the shared pool remains
capped at four workers.

| Median measurement | Original walker | Prepared policy |
|---|---|---|
| Scan wall time | 212.961 ms | 128.187 ms |
| Scan CPU time (`getrusage`, user + system) | 529.763 ms | 220.761 ms |
| First root progress | 88.315 ms | 81.025 ms |
| First measured item | 195.055 ms | 111.717 ms |
| Resident bytes immediately after scan | 17,399,808 | 17,170,432 |
| Resident growth across scan, bytes | 32,768 | 49,152 |

This fixture's wall time fell 39.81% and CPU time 58.33%. RSS comes from
`sysinfo::Process::memory()` in bytes, is an instantaneous reading rather than
peak working set, and establishes no memory improvement. The existing Stop
fixture returned in less than its one-millisecond resolution for both walkers.

Three full-catalog read-only observations per walker remained partial: original
24.148–27.254 s, current 22.150–28.012 s. They used the Codex shell host's existing
privacy grants and an unsigned diagnostic child, with background activity and
changing inventory; the current diagnostic also measured JSON event encoding.
These observations establish no live speedup or installed-app TCC behavior.
Current event encoding alone cost 59–95 ms for about 1.89 MB, not native IPC
latency. Homebrew preview, Codex runtime measurement and browser-use probes
remain the leading live spans; no use verdict is cached. The frontend burst
regression exercises real store callbacks and Stop dispatch with mocked IPC in
Node, without DOM rendering or native-window evidence.

### Full controlled matrix, 2026-10-01

The [sanitized matrix record](validation/2026-10-01-scan-matrix-0.3.100.json)
compares 0.3.96 production source `63a8a3e` with `00ca207` plus this change,
still labelled 0.3.100 before the final version sync. The same debug harness
is installed in both sources; retained binaries, harnesses and production files
have recorded SHA-256 fingerprints. This is the cumulative comparison across
the earlier shipped optimizations and current patch, not attribution of every
difference to this patch.

On MacBook Air M1, 16 GiB, macOS 27.0.1 (26A434), three alternating processes
per source/case warmed once and measured five scans: 15 samples per source/case.
The shared heavy-build lock was held; filesystem caches were warm and ordinary
host/browser activity continued. All 324 scans, grouped by fixture case, retain
equal inventory, eligibility, bytes/ranges, skipped/incomplete reasons and
ordering; all 54 boundary-Stop inventories and event-kind sequences also match
per case. No user directory or cleanup plan is used. Every original duration
ceiling and task bound is retained.

The committed disposable harness can be replayed with:

```sh
cargo test -p neati-desktop --test scan_benchmark controlled_profile \
  -- --ignored --nocapture --test-threads=1
```

The paired archive uses `--exact controlled_profile::<case>` on each retained
binary under `/usr/bin/time -l` so process peak RSS belongs to one fixture case.

| Fixture | Median wall ms, before → after | CPU delta ms, before → after | First item ms, before → after |
|---|---|---|---|
| wide | 182.567 → 147.479 | 297.146 → 180.137 | 173.841 → 139.903 |
| deep | 139.302 → 131.000 | 140.244 → 132.270 | 137.829 → 129.783 |
| mixed_size | 128.679 → 127.212 | 128.726 → 126.658 | 128.226 → 126.712 |
| mixed_age | 130.425 → 154.114 | 129.731 → 151.482 | 129.773 → 153.213 |
| aged_observation | 258.242 → 262.251 | 258.054 → 260.518 | 241.080 → 245.464 |
| plain_observation | 281.255 → 180.622 | 634.901 → 277.860 | 261.168 → 163.184 |
| overlap | 126.717 → 131.111 | 127.029 → 130.803 | 125.625 → 130.146 |
| link | 131.858 → 129.109 | 131.975 → 129.060 | 131.396 → 128.686 |
| inaccessible | 134.281 → 131.446 | 134.339 → 131.128 | 133.775 → 130.937 |

Root progress, full ranges, scan-end RSS and raw process-peak RSS bytes are in
the record. Peak outstanding tasks remain at most 16. Boundary Stop took
68–226 µs; separate active aged, unit-use and owned-child fixtures exercise
mid-work Stop. Whole-process peak RSS includes setup, warmups, output and Stop;
scan-end RSS is instantaneous. These data establish no memory improvement.
Small-fixture medians sometimes worsened and ranges overlap. The fresh process
snapshot dominates those current scans; ordinary background activity continued.
No outlier was removed and no broad scan speedup is inferred.

### Profiling decisions and read lifecycle audit

The aged walk remains one pass for size/newest-age/per-entry stale policy.
Child classification now reuses its own no-follow metadata for the link query;
Windows reparse failures remain protected. Only immutable per-walk path policy
and per-overlap-pass relationship facts are reused. No unit metadata, identity,
use verdict or cleanup authorization survives into planning/execution.

Current aggregate-only live observations were 28.918/27.424/27.145 s, partial,
with changing inventory, existing Codex-shell privacy grants and an unsigned
diagnostic child. Developer-temp took 77–88 ms: classification 1–2 ms, metadata
4–5 ms, tree traversal/policy 68–79 ms. Homebrew prefix/version reads took
39–65/39–45 ms, while its dry-run took 4,320–4,459 ms. npm/pnpm discovery took
182–194/232–249 ms; overlap resolution 1,379–1,434 ms and physical audit
508–509 ms. Nested spans are not additive. These diagnose remaining costs and
establish no causal live improvement or installed-app permission behavior.

Signature/category/provider orchestration retains its ordered single-writer
progress sink. Parallel orchestration would require buffering/interleaving that
stream and scheduling around nested walks and child-process admissions. Existing
independent browser-unit fanout uses the shared four-worker pool and keeps each
unit's fresh use verdict. Neither that pool nor the 16-task queue was increased.
One mutable Homebrew preview dominates startup; caching it would carry an old
mutation scope into a later read. Provider discovery performs one query per
provider, and none of its output replaces fresh prepare/execute checks.

| Read/recurring work | Audit outcome |
|---|---|
| Application inventory | Explicit view load/refresh; storage-read admission and blocking worker; no recurring collector. |
| Model/container inventory | Concurrent view readers now share only an in-flight read; post-mutation refresh waits then reads anew; errors/retry covered. |
| Developer/large-file scans | Explicit service-owned bounded reads, blocking workers and lifecycle cancellation; no automatic polling. |
| Development listeners | Existing in-flight sharing retained; one idempotent subscription per visible view now prevents duplicate reference counts and foreign releases. |
| Metrics and AI | Existing single-flight reads and keyed bounded backend snapshots retained; recurring view consumers own release and stop while hidden. |
| Awake/plan clocks | Awake's countdown/state-read timers and application/large-file plan clocks now stop while hidden and refresh on return; native Keep Awake evaluation and backend plan expiry retain their own lifetimes. |

Fake-timer/store regressions cover hidden/visible transitions, duplicate release,
multiple consumers, in-flight sharing, failure/retry and post-mutation freshness.
The associated developer-artifact clock change belongs to its workflow patch.
Rust callback/encoding measurements and synthetic mounted DOM evidence do not
measure native IPC. Actual native large-scan input/Stop and VoiceOver announcement
acceptance remains pending. Computer Use connectivity was subsequently restored,
but final-bundle native IPC/input/Stop and VoiceOver checks have not yet run.
No native criterion is waived; #379 remains open until that evidence is obtained.

## Recorded real-machine baselines

Counts are properties of the fixtures and are identical on every machine; the
numbers below are what the machines measured, recorded so a future run can be
compared against a known point rather than against a feeling.

| Machine | OS build | Date | neati | Baseline |
|---|---|---|---|---|
| MacBook Air (Apple M1, 8 cores) | macOS 27.0 (26A428), Darwin 27.0.0 | 2026-09-19 | 0.3.36 | the table below |
| MacBook Air (Apple M1, 8 cores, 16 GiB) | macOS 27.0 (26A428) | 2026-09-21 UTC | 0.3.45 | [synthetic and live evidence](validation/2026-09-21-macos-0.3.45.json) |
| MacBook Air (Apple M1, 8 cores, 16 GiB) | macOS 27.0 (26A428) | 2026-09-22 UTC | 0.3.48 | [filesystem, provider, container, and cancellation evidence](validation/2026-09-22-macos-0.3.48.json) |

macOS, `cargo test -p neati-desktop --test scan_benchmark -- --nocapture`:

```text
fixture            duration_ms  visited  directories  peak_tasks  candidates  skipped  logical_bytes  on-disk_bytes
wide                        68      801          201          16           1        0        2560000         3276800
deep                        59       35           33           2           1        1           4096            4096
mixed_size                  57        8            2           2           1        0        3163827         3174400
mixed_age                  174        7            3           0           2        0          16384           16384
overlapping_roots           58        6            3           2           1        0          12288           12288
inaccessible                97        3            2           2           1        1           4096            4096   (unix only)
symlink                     96        4            2           2           1        0           4155            4096   (unix only)
cancellation                60        2            1           1           1        0           4096            4096   (cancelled)
```

Windows CI regenerates the portable fixture baseline and runs the native
junction tests. This is Windows runner coverage, not a supported Windows 11
desktop baseline. The former numeric Windows table omitted its exact OS build
and run link, so it is no longer presented as reproducible machine evidence.
The remaining desktop checks are tracked in [WINDOWS_VALIDATION.md](WINDOWS_VALIDATION.md).

Two byte populations are reported, and only one of them is a committed fact:
`logical_bytes` is a property of the tree and is identical on every machine,
while the on-disk population is the platform's answer (APFS rounds each file up
to a block; `GetCompressedFileSizeW` reports what a file actually occupies, and
a small resident file is reported at its logical size). The benchmark commits
the logical population and bounds the on-disk one per fixture.

The rows also carry `rss_growth_kib`: this process's approximate resident-set
growth across the scan. It is reported and never asserted — the allocator
decides when pages return to the operating system, so two identical runs
disagree, and most of the growth belongs to building the fixture rather than to
walking it.

The `wide` row reaches `peak_tasks` 16, which is the bound being exercised
rather than trivially satisfied; the `cancellation` row shows the work actually
stopped (one directory read, no later candidate) and the result states
`cancelled: true`.

## Windows-specific behaviour

Reparse points, junctions, locked files, and sparse/compressed accounting are
stated in [WINDOWS_VALIDATION.md](WINDOWS_VALIDATION.md); the traversal rule is
that the walk uses one classifier (`SymlinkGuard`) for links and reparse points
in every walk, so a junction is a boundary in the walker and in the size
measurement alike rather than only in one of them.

## Repeating a read-only real-machine scan

Run the synthetic suite above first, then the explicit live observation tool:

```sh
cargo run -p neati-desktop --example scan_machine -- --live-read-only

# Add fixed-argument tool-provider and container inspection. These modes still
# construct no cleanup plan and invoke no prune/delete operation.
cargo run -p neati-desktop --example scan_machine -- \
  --live-read-only --providers-read-only --containers-read-only

# Stop provider measurement immediately after its first root progress event.
cargo run -p neati-desktop --example scan_machine -- \
  --live-read-only --providers-read-only --providers-cancel-after-first-root

# Inspect the full embedded catalog with the app's native provider registries.
# The optional private ledger includes paths and must stay in an owner-only file.
umask 077
cargo run -p neati-desktop --example scan_machine -- \
  --live-read-only --full-catalog-read-only --private-ledger \
  > /tmp/neati-full-private.json
```

By default the tool scans a fixed subset of the shipped filesystem catalog
with intensive observation enabled. Full-catalog mode uses the native
lifecycle and owner provider registries, and still only observes. Provider
inspection is opt-in and runs only the reviewed
cache-directory discovery commands before measuring the approved roots;
container inspection is opt-in and runs fixed status/list commands plus local
OrbStack metadata. It constructs no cleanup plan and invokes no cleanup
executor, provider prune, container prune, delete, or Trash operation. A
cooperative 180-second filesystem-scan deadline returns a cancelled/partial
result; it is not an OS-level timeout for a stalled filesystem call. Default
output contains catalog IDs, typed gaps, and aggregate counts/bytes, never
item paths, user names, tool output, free-form errors, or container identifiers.
The explicit `--private-ledger` flag adds item paths and reasons for local
investigation; never commit or share that output. Record the OS build,
hardware, date, and version alongside the JSON. A zero-item signature is
missing coverage, not a passing application-specific check.

The [0.3.45 Mac record](validation/2026-09-21-macos-0.3.45.json) contains both
synthetic fixture results and an actual cache scan. The live result was partial:
sandbox access refusals remain visible in the incomplete/skipped totals. No
cleanup ran. Cursor/Go produced no items, so that run does not establish their
real-machine behavior. Timings include background developer activity and are
observations for comparison, not new pass/fail thresholds. Windows 11 desktop
validation and the uncovered Mac application/provider cases remain open in
#224/#227; #221 cannot close on this evidence alone.

The [0.3.48 Mac record](validation/2026-09-22-macos-0.3.48.json) adds pnpm/npm
cache measurements, provider progress and cancellation, a typed uv inspection
failure, Docker-daemon availability, and OrbStack aggregate metadata. The
filesystem observation is still partial, Docker was not running, and
Cursor/Go still produced no items. The evidence classes and remaining closure
boundary are summarized in [VALIDATION_SIGNOFF.md](VALIDATION_SIGNOFF.md).
