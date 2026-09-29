# Scan coverage and accounting: issue #363

This change targets `develop` at `0cb61fa` and advances 0.3.80 to 0.3.81.
The companion cleanup adapters remain in #362 / PR #364. This change does not
implement their operations or the privileged helper. CLI #335 and Xcode #350
remain separate.

## Acceptance ledger

| Issue requirement | Implementation and evidence |
| --- | --- |
| Classify discovered namespaces once | `scanner/coverage.rs` observes excluded direct namespaces and include-list misses inside catalog roots. Complete exact owner observations replace synthetic rows; unresolved overlap retains both actions with a range. Ordinary candidates retain their original policy. |
| Bound uncovered observations | At most 64 excluded roots per root, 4,096 entries / 50 ms / 16 levels per observation. Missing/inaccessible entries, links, cancellation and limits retain partial observations, including zero measured bytes. |
| Physical identity and containment | Existing identity-aware containment remains; hard-link entries and nested shared files now qualify totals without merging distinct operations. A 50,000-entry / 500 ms audit reports uncertainty when incomplete. Scan and merged-slice results both retain the gap. |
| Representative fixtures | `tests/fixtures/coverage/names.json`, coverage routing tests, shared-storage tests, existing selector/containment cases, and the temporary-prefix regression. These use disposable fixture roots. |
| Shared explanations | `cleanupGuidance` supplies typed owner, cause and destination to Storage and Settings. Review routes to the category; owner review uses the existing quit flow; container/model observations route to their owner screens. Unsupported operations explicitly say no cleanup action. |
| Scan through outcomes | The 1.5 MB ready / 1.3 GB busy / 5.9 MB review regression verifies selection, category amounts, separate permanent/Trash outcomes and remaining rows. |
| Verified-idle investigation | No remaining verified-idle misclassification reproduced in the tested scope. The Parsecd fixture keeps a running process and varies exact handle state (idle/busy/unknown); browser fanout preserves each unit verdict. Live Chrome HTTP caches remained owner-gated, while CacheStorage units were reviewable rather than inferred busy solely from Chrome presence. |
| Distinct outcomes | Results independently show permanent removal and moved-to-Trash bytes, including when one operation reports both. Provider mechanism remains separate. Zero, negative and unavailable free-space readings remain distinguishable. Trash emptying remains the companion workflow and never enters cache candidate totals. |
| Accounting and authority | Existing item/category/overall invariants remain; synthetic IDs are unregistered and selections cannot authorize deletion. The safety integration test for an observed protected namespace forges selection and verifies plan rejection. Unknown provider estimates stay unknown. |
| Profiling and optimization | Measured Chromium unit probes now use the existing bounded four-worker pool, retaining a serial fallback and fresh planning/execution checks. Controlled serial/parallel comparison and sequential live reports are in the validation directory. |
| Mole comparison | 118 preview paths classified against the candidate's private ledger; sanitized counts, same-user execution conditions, tool versions and unmatched scope are recorded. No real cleanup was run. |
| UI and delivery evidence | Six viewport/theme screenshots, shared-action navigation checks, required local checks and bundle inspection are recorded in `validation/scan-363/README.md`. |

## Measurement boundaries

Observation is not authority. Synthetic `coverage:` IDs cannot resolve to a
registered cleanup signature. They never create a filesystem-delete fallback.
Full owner observations may absorb an exact synthetic footprint; a partial owner
cannot silently erase an independently measured observation.

This is a catalog-scoped inventory, not a home-wide filesystem census. Unrelated
temporary children are intentionally ignored even for read-only observation.
Known temporary prefixes retain their age and structured-state checks. Fixed
catalog exclusions still defer to their registered owner; deeper pruned entries
remain represented by their containing observation's skipped/partial coverage.
Links and mount points are not traversed. Complete zero-byte rows remain omitted.

Hard links preserve distinct cleanup operations. The physical audit qualifies
possible shared storage; it does not claim exact per-unit reclaimability, detect
APFS shared extents, or change a plan's permissions. If the audit reaches its
budget, the scan is partial and the observed-byte range may be broad. Neither
that range nor a Mole preview is an actual free-space result.

## Tracking

Implementation and local evidence belong to this PR. #363 remains open until
review and merge; the ledger records delivered behavior, not a merged release.
The signed-helper prerequisite remains tracked by #362. Installed applications
are not replaced by building this branch.
