# GPU cache coverage and the remaining Mole gap

Issue [#314](https://github.com/jaeyoung0509/neati/issues/314), based on develop
`7c7a1a8` (v0.3.64), implemented as v0.3.65. The product priority is to close
verified cleanup coverage gaps against the **installed `mo clean`**, not to
accumulate signatures or improve a displayed total without an executable action.

## Implementation and limits

- Three exact GPU cache names for Codex, Antigravity, and Cursor, each scoped
  separately to macOS or Windows default application-data roots.
- Running-owner checks apply to every declared process guard at scan, planning,
  and execution, including Windows process-name snapshots. An unreadable process
  table does not mean idle. Codex also guards the locally observed ChatGPT host
  and `Codex (Renderer)` / `Codex (Service)` helper names.
- Generic structured-state guards remain intact. These entries do not inherit
  the relaxed database/lock policy of verified renderer caches.
- Unknown application owners remain Manual observations. Exact root exclusions
  keep those rows from shadowing or double-counting the three known owners.
- The private diagnostic ledger now includes the disposition reason, so a fully
  measured but blocked provider unit explains its refusal. Private paths and
  free-form reasons are not added to the normal aggregate report.

The upstream directory semantics and consequences are documented in
[CACHE_SUPPORT.md](CACHE_SUPPORT.md#chromium-application-gpu-subtrees). No Mole
code, tests, or catalog tables were copied. No frontend design, native glass,
capability grant, or dependency changes are included.

## Before and after: read-only full scans

Host: Apple M1, 16 GiB, macOS 27.0 (26A428), September 27, 2026. Full embedded
catalog, native providers, intensive observation enabled, no exclusions, normal
user privileges. Background applications stayed active; filesystem caches were
not flushed. These single uncontrolled samples establish coverage, not a speed
claim. No live user cache was removed.

| Measurement | v0.3.64, 01:54:16 UTC | v0.3.65, 02:10:08 UTC |
| --- | ---: | ---: |
| Observed bytes | 4,478,689,280 | 4,456,230,912 |
| Cleanable bytes | 1,283,825,664 | 1,221,074,944 |
| Selected bytes | 1,126,449,152 | 1,126,576,128 |
| Scan duration | 6,185 ms | 6,948 ms |
| Visited entries | 23,354 | 24,859 |
| Directories read | 4,163 | 4,420 |
| Quality | partial | partial |

The new GPU signatures retain **three Codex units totaling 1,683,456 allocated
bytes**. They are `reviewable`, not selected, because their owner is active.
The current domain's cleanable bucket includes reviewable units; it is not a
promise that planning will succeed while the owner is running. A fresh planner
check refuses that condition. The other new owner/advisory signatures retained
zero units on this snapshot. Total byte changes also reflect a changing live
filesystem and broader enforcement of existing process guards.

**This addition is small and does not solve the main Mole gap.** Its local
contribution must not be described as a large reclaim or performance gain.

## Paired comparison with installed Mole

Installed CLI: `/opt/homebrew/bin/mo`, Homebrew Mole **1.55.0**. The command was
`mo clean --dry-run` with `MOLE_DRY_RUN=1`, non-interactive, with no sudo request.
Mole reported system cleanup skipped and completed its preview at **02:09:53
UTC**, 15 seconds before neati's post-change scan began. The raw preview and
neati private ledger stayed outside the repository with owner-only permissions.
The installed `bin/clean.sh` SHA-256 was
`5d4c864ed8afaa67216dbff6a41dca9ae44735bcd353a1db8b722c69e4b46d04`;
no upstream tag was assumed to prove these locally installed bytes.

The existing `scripts/cleanup_coverage_audit.py` matched paths and measured
allocated footprint using bounded `du -skP` after both scans. The preview is
not a record of actual removal; neither scan establishes a live disk-free delta.

| Comparison | Result |
| --- | ---: |
| Mole displayed potential | 2.96 GB, 401 entries, 5 categories |
| Mole listed paths without a neati match | 365 |
| Exact / neati ancestor / neati descendants matches | 15 / 17 / 4 |
| Structurally nested preview rows | 2 |
| Measured allocation of non-nested preview rows | 2,893,717,504 bytes; 1 row unmeasured |
| neati selected bytes | 1,126,576,128 |

The two nested rows do **not** explain away the discrepancy. The largest groups
are missing scope or a provider refusal:

| Group | Measured allocated bytes | Current neati state | Follow-up |
| --- | ---: | --- | --- |
| Browser Service Worker CacheStorage | 947,761,152 across 84 rows | Not in the catalog; offline website assets require a distinct owner-backed action, not generic GPU cleanup | [#318](https://github.com/jaeyoung0509/neati/issues/318) |
| DotSlash artifacts | 537,231,360 across 2 artifacts | Observed, both blocked by the required owner-lock file being absent | [#316](https://github.com/jaeyoung0509/neati/issues/316) |
| Browser component download cache | 181,391,360 across 29 rows | `component_crx_cache` is not in the catalog | [#317](https://github.com/jaeyoung0509/neati/issues/317) |

These groups total **1,666,383,872 measured bytes**, not a guaranteed reclaim
amount. DotSlash is the first implementation priority: a 02:12:22 UTC read-only
diagnostic states `The artifact's owner lock is missing or is not a regular
file` for both units, and metadata inspection confirms the default cache has
two hash-addressed artifacts but no `locks` directory. Its real lock lifecycle
must be verified rather than bypassed.

Most of Mole's large sandbox-cache group is already covered: one exact temporary
cache unit contributes **1,069,674,496 selected bytes** in both path ledgers.
It is not a new missing gigabyte. Cookies, sessions, Local Storage, and offline
state are not made disposable by being near that covered unit.

## Verification

Local macOS: **1,124 Rust tests passed**, four existing exporters ignored;
**402 frontend tests passed**; Svelte check reported zero errors/warnings.
Workspace compile, strict Clippy, formatting, architecture, frontend production
build, and synchronized version checks passed. The added native-platform
fixtures cover process state changes, exact owners, unknown owners, profile
siblings, protected descendants, root replacement, and links/junctions.
Recoverable moves use a temporary fixture Trash, never the operating system's
real Trash or user directories.

Version bumped once: **0.3.64 -> 0.3.65**. No release, tag, merge, or installation
is part of this work. Windows runtime/junction results are reported separately
by PR CI; local macOS checks do not establish Windows behavior. There is no new
UI/asset design requiring screenshots.

`just build-fast` completed. The standalone bundle's short/build versions both
read **0.3.65**, its packaged `icon.icns` matches the tracked icon byte-for-byte,
and the rendered packaged icon was inspected. The bundled binary's `--doctor`
self-check passed. The installed/running application was **not replaced**;
native interactive window behavior was not rerun.
