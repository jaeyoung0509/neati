# neati vs Mole: real-machine scan benchmark

2026-09-29 · macOS 27.0 (26A428) · Apple silicon.

**Result:** neati scanned faster in this setup. Mole displayed a larger potential total, but about 194 MB was represented by nested parent/child paths. neati also has an overly broad owner-verification failure introduced in merged PR #355 and tracked in #356.

## Method

- neati 0.3.77, commit `3bd3600`, merged PR #355. Built the existing `scan_machine` example, which calls the production ScanEngine and native providers. This was a Cargo **dev build**, not an optimized release benchmark; compilation was excluded from timing.
- Installed Homebrew Mole 1.55.0: `mo clean --dry-run`.
- neati command: `target/debug/examples/scan_machine --live-read-only --full-catalog-read-only --private-ledger`.
- Three sequential runs each, ordered M1 → N1 → N2 → M2 → M3 → N3. No competing build jobs, no cache flush, no application termination. Live applications continued changing their caches.
- No cleanup/deletion commands ran. Mole's previous preview file was backed up and restored.
- Mole retained its 19 core protection patterns and skipped privileged system preview because no sudo session was available. neati used the full catalog with extended scope enabled and no signature exclusions. These scopes are not identical.

## Measurements

All sizes below use decimal MB/GB, matching Mole's display units.

| Metric | neati | Mole |
| --- | ---: | ---: |
| Run 1 | 20.376 s | 30.532 s |
| Run 2 | 18.162 s | 26.769 s |
| Run 3 | 19.335 s | 28.044 s |
| Median | **19.335 s** | **28.044 s** |
| Reported cleanup candidates | **361.5–364.7 MB** | **609.0–610.4 MB potential** |
| Default-selected bytes | **30.2 MB** | No equivalent selection result in this preview |
| Observed footprint, including retained data | About **5.53 GB** | Not reported as a comparable inventory |
| Coverage status | **Partial**, no cancellation | Completed user-level preview; system scope skipped |

neati's median was 31.1% shorter for these invocations. This does not establish release-build performance or deletion throughput. neati's cleanup candidates include reviewable/running-owner entries; they are not all immediately executable. Its observed footprint includes about 3.40 GB of partially measured Codex installed runtimes with zero cleanable bytes.

## Why Mole's displayed total is larger

Each preview contained 222 rows. Seven rows were descendants of other listed rows:

- Homebrew cache root plus a large archive inside `downloads`: about **129.2 MB** overlaps.
- Google cache root plus Chrome's `Default` subtree: about **64.4–65.4 MB** overlaps.
- Other small nested paths account for the remainder.

Summing only the top-level, non-nested preview rows gives **415.4–415.7 MB**, rather than 609.0–610.4 MB. This is a correction to rounded path-footprint accounting, **not a verified reclaim estimate**. Mole's installed `render_clean_preview_from_ledger` adds each retained row's size; its exact-identity deduplication does not remove these ancestor/descendant overlaps.

A separate `du -skP` pass after timing measured **428.7 MB** across those disjoint roots, with two roots not measurable. The difference from the preview is consistent with live cache changes and rounding; it is not proof of additional reclaimable bytes. Several owner-command actions (npm, pnpm, bun, pip, uv, GitHub CLI and Homebrew) were displayed as “would clean” without a corresponding measured prune yield.

## Coverage comparison

196/222 Mole rows had an exact, ancestor or descendant relationship to a neati observation. That establishes path coverage only, not equal authorization. The 26 unmatched rows summed to approximately **1.57 MB** in the rounded preview; a later allocated-size measurement found approximately **1.54 MB**. The largest was a Google Updater old log (1.1 MB), followed by diagnostic reports, shell completion dumps and project Python bytecode.

The large common populations were Homebrew downloads, Cargo archive cache and Chrome HTTP/code caches. For example, Mole's Homebrew root includes about 51.7 MB of metadata that neati observes as advisory. Keeping settings/metadata and applying log retention also explains part of the residual difference.

## Confirmed neati problem: global unknown owner state

The actual process snapshot contained `liquiddetectiond.app`, a system process bundle whose expected `Contents/Info.plist` was absent. In `src-tauri/src/applications/mod.rs`, a bundle metadata read failure clears the single `bundle_state_known` flag; `running_executables` then returns unknown for unrelated inferred owners without a positive match.

Evidence in all scan ledgers:

- **21 generic user-cache items / about 32.05 MB observed** were blocked with “Owner process state could not be verified.” These are observed bytes, not a claim that all 32 MB can safely be removed.
- Across all scopes, **334 items** received that owner-verification reason, many with zero measured bytes.
- The scan also reported 29 permission-denied and 147 Full Disk Access gaps. The overall 341 `io_error` gap count includes the owner-verification failures and must not be interpreted as 341 filesystem read errors.

**Follow-up #356:** inspection found a flat system executable container, not a conventional application bundle. Distinguish that layout from genuinely unreadable app metadata; keep executable and bundle-name guards for the process. Genuine conventional bundle failures continue to fail closed. Add layout/lookalike regression fixtures and rerun the scanner. General per-owner uncertainty and typed gap reporting remain future work.

This baseline ran before the #356 fix and exposes a layout the original temporary-fixture tests had not represented. No cleanup was executed.

## Files

- [Machine-readable summary](baseline.json): aggregate timings, totals and limitations, without user paths.
- Raw stdout/stderr, individual Mole previews and neati private ledgers remain in this private local directory. They are not committed or uploaded.

## #356 remeasurement before Full Disk Access

The same read-only neati scanner was run three times on 0.3.78 after the flat
system-container fix. [Aggregate results](after.json): 21.831, 18.228 and 18.033
seconds (median 18.228). All three reported 415,136,448 conditional cleanable
bytes and 55,988,224 selected bytes; coverage remained partial.

Unknown-owner items fell from 334 to zero. Of the original blocked population,
328 entries still appeared: 13 were automatic, 5 recent, and 310 remained
blocked. Those entries now contribute 25,796,608 selected bytes. IO gaps fell
from 341 to 7; access gaps became visible after the earlier owner failure no
longer masked them (31 permission-denied and 454 Full Disk Access gap records).
These counts are observations, not unique physical directories.

Live caches changed between measurements. Mole was not rerun for this focused
regression check; no cleanup or actual disk recovery was measured.

## Comparison after protected-directory access succeeded

The user enabled macOS access permissions. Before measurement, this execution
host successfully enumerated Safari cache, Safari Library and Mail directories
that previously returned EPERM. Both tools ran under the same Python runner
from the Codex shell host; the exact responsible TCC app identity was not
independently resolved. This is evidence of effective access, not an assumption
that an installed app's grant transfers to every CLI. Retention thresholds,
owner protections and tool configuration stayed unchanged.

[Full aggregate evidence](full-disk-access.json), three sequential runs per tool:

| Metric | neati 0.3.78 | Mole 1.55.0 |
| --- | ---: | ---: |
| Run 1 | 17.074 s | 42.732 s |
| Run 2 | 14.345 s | 41.305 s |
| Run 3 | 15.787 s | 39.447 s |
| Median | 15.787 s | 41.305 s |
| Reported candidates | 494.636 MB conditional | 1.34 GB potential |
| Default selected | 66.040 MB | No equivalent selection result |
| Observed | About 7.333 GB | No comparable inventory |

All six runs exited successfully. neati had zero Full Disk Access gaps and
zero unknown-owner items. Five permission-denied and seven IO gap records
remained, so coverage is still partial. Full Disk Access is not administrator
access: Mole still skipped privileged system preview and requested `sudo -v`.

Mole's 517 preview rows contained 10 nested overlaps (~226.581 MB). Disjoint
rounded top-level footprints totaled 1.1092–1.1093 GB. neati already observed
613.732 MB of Chromium offline CacheStorage that stayed blocked because Chrome
was running. This accounts for much of the difference from its conditional
cleanup estimate. Mole lists matching subtrees in dry-run; this does not prove
that its actual execution would remove all of them.

417 Mole rows overlapped neati observations. The 100 unmatched rows totaled
24.577 MB in rounded preview sizes, including ~14.704 MB under containers and
small log/other scopes. These observations update #357; owner and temporary-file
contracts remain necessary before adding deletion coverage.

No application was stopped and no cache deletion ran. The prior Mole preview
file was restored and verified. These are live-system dev-build scan timings,
not release performance or measured disk recovery. Private ledgers and previews
stay local; only aggregate evidence is committed.

## #359: ordinary cleanup for verified download archives

The user then requested Mole-aligned policy. Ready Homebrew downloads and Cargo
registry archives now enter ordinary cleanup without a second confirmation,
using their existing owner adapters. Retention thresholds and stateful-store
review were unchanged. [Installed source hashes](mole-source.json) identify the
Mole files inspected for this policy decision.

[Three final scans](policy-defaults.json), with the same effective permissions,
reported 529,877,696 conditional cleanable bytes and 331,907,072 selected bytes.
Scan times were 16.693, 14.278 and 14.064 seconds (median 14.278). Coverage
remained partial with 5 permission-denied and 7 IO gap records.

| Download population | Observed/cleanable bytes | Selected before | Selected after |
| --- | ---: | ---: | ---: |
| Homebrew downloads | 129,212,416 | 0 | 129,212,416 |
| Cargo registry archives | 105,439,232 | 0 | 105,439,232 |
| Total | 234,651,648 | 0 | 234,651,648 |

The total selected amount changed from 66,039,808 to 331,907,072 bytes. Only
234,651,648 bytes are directly attributable to this policy change; Help and
container eligibility also changed as live process state changed. The aggregate
cleanable delta is not evidence that this patch expanded filesystem scope.
Mole was not rerun for this focused policy comparison. No real cleanup ran.

Final local verification: `cargo check`, `cargo test` (1,184 passed, 5 ignored),
owner-provider tests (51 passed), Clippy with warnings denied, format, architecture,
frontend typecheck, Vitest (434 passed), Vite build, bindings (no drift), version
synchronization and `just build-fast` all passed. The final 0.3.78 app's icon
and embedded frontend were verified. It was not installed or launched to replace
the user's app. Windows runtime behavior remains manually unverified.
