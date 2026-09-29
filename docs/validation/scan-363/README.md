# Issue #363 validation

Run: **2026-09-29**, Apple Silicon, **macOS 27.0 (26A428)**.
Baseline: `develop` commit `0cb61fa`, **0.3.80**.
Candidate: this PR, **0.3.81**. Mole: **1.55.0**, Homebrew installation.
[Acceptance ledger](../../CLEANUP_363_SCOPE.md).

## Read-only live comparison

The final pair ran sequentially from the same execution host without sudo,
with no build/test jobs between the two scans. Both used
`scan_machine --live-read-only --full-catalog-read-only`. The candidate also
emitted a private ledger for local path matching; that ledger is not committed.
Both scans used the full catalog and native owner previews. No app was quit and
no cache cleanup was executed. This host is not an isolated filesystem snapshot.
OS denials remain visible; Full Disk Access parity with the installed GUI is not
asserted. No permission was granted or changed for this run.

| Measurement | Baseline | Candidate |
| --- | ---: | ---: |
| Scan duration | 33,815 ms | 28,016 ms |
| Observed row sum (upper bound) | 10,873,244,832 B | 12,353,133,728 B |
| Cleanable row sum | 3,629,591,712 B | 3,629,591,712 B |
| Default selection | 213,688,320 B | 213,688,320 B |
| Retained row sum (upper bound) | 7,243,653,120 B | 8,723,542,016 B |
| Directory reads | 10,893 | 13,294 |
| Visited entries | 98,164 | 110,972 |
| Incomplete items | 12 | 15 |
| Permission-denied gaps | 5 | 5 |
| Scan quality | Partial | Partial |

Raw sanitized reports: [baseline](baseline.json), [candidate](candidate.json).
The candidate's observed union is qualified as **1,141,067,776–12,353,133,728 B**.
This intentionally broad range includes unresolved containment and a bounded
physical audit. Its lower endpoint is not cleanable capacity; item permissions
and operation estimates are separate from unique physical accounting. Additional
observations do not create additional deletion permissions. The unchanged
cleanable/selected sums provide a useful check for this pair, not proof that
all future scans will return the same values.

| Profile span | Baseline | Candidate |
| --- | ---: | ---: |
| Chromium offline units | 7,218 ms | 2,400 ms |
| Generic user caches | 5,556 ms | 5,523 ms |
| Codex runtime observation | 4,482 ms | 4,117 ms |
| Homebrew owner preview | 4,450 ms | 3,529 ms |
| Cache-provider collection | 3,345 ms | 3,257 ms |
| Container caches | 1,664 ms | 1,450 ms |
| Application logs | 320 ms | 317 ms |
| Trash observation | 0 ms | 0 ms |
| Added physical audit | — | 507 ms |

Trash produced no nonzero row in either run; this is not a benchmark of a
populated Trash tree. The physical audit checks a 500 ms budget between entries,
so wall time can exceed it during an in-flight filesystem call. Profile spans
include nested/concurrent work and must not be summed as exclusive CPU time.
Earlier exploratory scans took 39.622 s before and 28.303 s after. Another
intermediate run took 31.790 s; it exposed an overbroad temporary observation
which was corrected before the final pair. Those runs are not included as
controlled replicates. The final pair was about 17% faster, but filesystem cache
warmth, content changes and other host processes prevent a causal speedup claim.

A controlled 12-unit fixture uses 5 ms probes with independent idle, busy and
unknown verdicts. The final run took **21 ms parallel / 77 ms serial**, with
**four peak probes** and identical unit paths, allocated bytes and states. This
verifies bounded fanout and preserved decisions; timings are recorded evidence,
not a flaky performance threshold in the test.

## Mole scope reconciliation

`mo clean --dry-run` exited 0 in **30.005 s**, reporting **5.07 GB potential**,
118 items and five categories. It explicitly reported that system caches need
sudo and excluded that privileged scope. Its generated preview list was read
and the pre-existing list was restored. No mutation command was executed.

[Sanitized comparison](mole-comparison.json):

- 70 preview paths exactly matched neati units: nine automatic, 54 reviewable,
  seven blocked. Matching a path does not prove identical deletion scope.
- 31 were inside a neati unit; three contained neati units.
- 14 had no matching unit: eight rounded to 0 B, while six covered GoogleUpdater
  metadata, three Oh My Zsh cache entries and two shell completion files. They
  remain outside the registered action coverage; this scan issue does not add
  owner adapters for them.
- Six preview rows were nested under another preview row. Summing the displayed
  preview entries is therefore not a unique-storage measurement.

Chrome CacheStorage matched particularly well: both neati runs observed
1,169,010,688 B, of which 1,168,809,984 B was reviewable and metadata stayed
advisory. Mole displayed about 1.17 GB for Service Worker cleanup. The neati
units require explicit confirmation; that is different from declaring them
busy simply because Chrome runs. HTTP/code caches still follow their existing
owner policy. Live data did not establish a verified-idle false-positive case;
the Parsecd and browser fixtures test idle/busy/unknown states independently.

Mole's potential-space figure, neati's observed footprint, selected capacity,
Trash movement and actual free-space change are different measurements. This
run provides no actual-removal or freed-space result.

## UI evidence

Browser preview with deterministic fixtures; **0.3.81**, same OS/date as above,
reduced motion enabled, light and dark themes. The existing design contract and
components were reused. The UI skill search supplied no verified specific
pattern for this case. All six captures were visually inspected: no horizontal
overflow, summary and footer agree at 1.5 MB, and retained items remain reachable
through the scrollable disclosure. This is browser evidence, not native-webview
or Windows execution evidence.

| Viewport | Light | Dark |
| --- | --- | --- |
| 800 × 560 | [Capture](storage-light-800x560.png) | [Capture](storage-dark-800x560.png) |
| 960 × 660 | [Capture](storage-light-960x660.png) | [Capture](storage-dark-960x660.png) |
| 1280 × 800 | [Capture](storage-light-1280x800.png) | [Capture](storage-dark-1280x800.png) |

Storage and Settings render the same owner/action labels. Activating the review
button from either view reached the System category with 1.5 MB selected; no
cleanup or process termination was activated. Result rendering is covered by
frontend tests for simultaneous permanent/Trash amounts and signed free-space
readings. AGENTS.md now explicitly exempts PRs without actual UI changes from
screenshots, as requested by the owner.

## Local verification

- `cargo test --workspace`: **1,217 passed**, five existing ignored tests.
- Final targeted physical-accounting tests: two passed; serial/parallel probe
  regression: one passed. These also cover the last accounting-bound adjustment.
- `cargo check --workspace`, `just lint-rust`, `just check-architecture`: passed.
- `just generate-bindings`: passed; no generated binding drift.
- `pnpm check`: zero errors/warnings; `pnpm test -- --run`: **443 passed**;
  `pnpm build`: passed.
- `just check-version`: synchronized **0.3.80 → 0.3.81**.
- `just build-fast`: standalone `.app` bundle built with current frontend.
  `Info.plist` reports 0.3.81; packaged `icon.icns` is byte-identical to the
  source icon (137,274 bytes). Bundle `Neati --doctor`: **14/14 checks passed**.
- Installed/running app was **not replaced**. No release or tag created.

CI is reported on the PR separately. Windows filesystem behavior and packaging
were not run on this Mac. PR #364 was still open at final verification; if it
merges first, synchronize this PR's version to the next patch before merge.

## CI fixture correction, 2026-09-30

The initial Windows Rust job failed the coverage routing fixture: it expected
three rows but received none. The fixture copied the macOS-only declaration from
the production signature, so Windows discovery filtered out both fixture rules.
The correction relocates the broad rule to its explicit temporary root and makes
both fixture rules platform-neutral. An assertion now checks that both rules
reach discovery before checking their accounting. Production catalog platforms
and cleanup permissions are unchanged. This follow-up retains version 0.3.81.
