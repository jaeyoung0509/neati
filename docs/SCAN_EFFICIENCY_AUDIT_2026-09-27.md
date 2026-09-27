# Scan efficiency audit, September 27, 2026

Issue [#313](https://github.com/jaeyoung0509/zenith/issues/313), based on develop
`050ec15` (v0.3.63). The implementation is v0.3.64. This audit focuses on the
scan/overlap/link-classification paths and candidate cache coverage; it is not
an exhaustive review of every product feature.

## Findings and disposition

| Finding | Disposition |
| --- | --- |
| Overlap resolution captures unit and ancestor identities repeatedly for every candidate pair. Equivalent entries also repeat parent-directory enumeration. | Reuse observations within one overlap pass, through the same relationship algorithm the uncached planner uses. Nothing is cached across scans or into authorization. |
| The size and age walkers stat ordinary entries twice: once for link classification, once for size/type. | Classify the already-read non-following metadata. Windows still queries reparse tags; classification failures report a partial measurement. |
| Scan spans omit overlap resolution and slice merging. | Add `scan.overlap_resolution` and `scan.overlap_merge`; include the current merge pass in merged duration. |
| External cache-provider work consumes roughly half this machine's full scan. | Still open under [#294](https://github.com/jaeyoung0509/zenith/issues/294). The group span combines discovery commands and measurement, so it does not establish which provider or subprocess is slow. Per-provider profiling should precede concurrency or persistent-cache changes. |
| Application-data selectors omit locally observed `DawnGraphiteCache`, `DawnWebGPUCache`, and `GrShaderCache` names. Generic named-subtree scans do not infer a running owner like enumerated namespaces or verified renderer contracts. | Track scoped discovery plus owner verification in [#314](https://github.com/jaeyoung0509/zenith/issues/314). No new deletion scope or signature is added here. |
| Per-entry blacklist checks rebuild environment-derived values, and exclusion checks repeatedly expand patterns. | Further profiling opportunity, not an established timing attribution. Any precomputation must preserve the stated environment and platform path semantics. |

No UI, native glass, cleanup policy, provider command, dependency, or permission
changes are included. The patch removes duplicated link-accounting branches
instead of maintaining separate file/link measurement paths.

## Deterministic evidence

The new all-pairs fixture compares 64 existing sibling directories through both
the live probe and the scan snapshot. On this machine, 4,096 comparisons need
**36,672 identity probes without reuse versus 71 with reuse**. The 71 comprise
64 candidates and seven distinct ancestors. The assertion uses the actual set
of paths, not a platform-specific constant. Directory-entry resolution is read
once per candidate, and a repeated pass through the same snapshot performs no
additional identity or directory reads.

This measures the isolated relationship algorithm, not the number of calls in
a full application scan and not a 99.8% overall speed improvement. Additional
tests cover hardlinks as distinct entries, real-volume case behavior, missing
observations, and identity replacement between observation and fresh checks.
The existing fixture scan accounting remains unchanged.

```sh
cargo test -p zenith-desktop scanner::relationship::tests -- --nocapture
cargo test -p zenith-desktop --test scan_benchmark -- --nocapture
```

## Read-only machine observations

Machine: Apple M1, 16 GiB, macOS 27.0 (26A428), September 27, 2026.
Both runs used the full embedded catalog with native providers and intensive
observation enabled. They constructed no cleanup plan and deleted no user data.
The pre-change run preceded compilation and tests; the post-change run followed
them. Filesystem caches were not flushed and background applications remained
active. These are single uncontrolled samples, not a cold/warm benchmark.

| Measurement | v0.3.63, 01:25:15 UTC | v0.3.64, 01:40:12 UTC |
| --- | ---: | ---: |
| Scan duration | 6,599 ms | 6,406 ms |
| Cache-provider group | 3,248 ms | 3,112 ms |
| Overlap resolution | not separately measured | 143 ms |
| Visited entries | 23,265 | 23,268 |
| Directories read | 4,163 | 4,163 |
| Peak outstanding directory tasks | 16 | 16 |
| Observed bytes | 4,402,860,032 | 4,477,411,328 |
| Cleanable bytes | 1,207,853,056 | 1,282,809,856 |
| Selected bytes | 1,126,449,152 | 1,126,449,152 |
| Quality | partial | partial |

The 193 ms difference is too small and uncontrolled to attribute to this patch.
Byte/entry changes reflect a live filesystem, not newly supported cache paths:
the catalog is unchanged. Neither observed nor selected bytes are actual
reclaimed space. The remaining partial observations are not hidden or relabeled
as successful scans.

To reproduce the observation without a private path ledger:

```sh
cargo run -q -p zenith-desktop --example scan_machine -- \
  --live-read-only --full-catalog-read-only
```

## Verification boundary

macOS local verification includes Rust check/tests, strict workspace Clippy,
formatting, architecture checks, Svelte checks, 402 frontend tests, and the
production frontend build. Rust's complete suite reports 1,113 passing tests;
four existing artifact-export tests are ignored by the ordinary suite.
The scan-baseline exporter was also run explicitly and produced no tracked
baseline changes. The disposable cleanup benchmark removed 64 fixture files,
leaving zero and reporting 4,194,304 reclaimed bytes against the fixture's
4,194,304 observed allocated bytes. This is test data, not user-cache cleanup.
Windows junction/reparse behavior must be verified by the PR's Windows job;
local macOS results do not establish Windows runtime behavior.

The version is bumped once from 0.3.63 to 0.3.64 through `just bump-patch`.
An app bundle build is not installation, and no running/installed app is
replaced by this audit. No release, tag, or merge is part of this work.
`just build-fast` completed; the resulting bundle's two version fields are
0.3.64, and its packaged `icon.icns` matches the tracked icon byte-for-byte.
The packaged icon was rendered and inspected. No UI/assets changed, so there
is no new visual design to validate; native window interaction was not rerun.
