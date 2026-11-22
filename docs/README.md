# neati documentation

This index separates documents for people using neati from the records that
maintainers use to make and audit decisions. Files keep their current paths
because `AGENTS.md`, tests and other documents link to them.

## Using neati

| Document | What it covers |
|---|---|
| [Cleanup Safety Model](SAFETY.md) | What neati will and will not remove, and where authorization is decided |
| [Cache and Runtime Support Matrix](CACHE_SUPPORT.md) | Which tools' caches are recognized and how each is cleaned |
| [System cleanup access](SYSTEM_CLEANUP_ACCESS.md) | Full Disk Access, protected locations and retained state |
| [AI Control Center](AI_CONTROL_CENTER.md) | Usage tracking, budgets and whether data is live, local or manual |
| [Project Cockpit](PROJECT_COCKPIT.md) | How AI CLI processes are grouped by project |
| [Identity contract](NEATI_MIGRATION.md) | Settings, credential and permission identities after the rename |
| [Windows development](WINDOWS.md) | Windows install modes and layout |
| [Code signing policy](../CODE_SIGNING_POLICY.md) | Signing and notarization status and how to verify a release |

## Architecture and contributor guides

| Document | What it covers |
|---|---|
| [Architecture](ARCHITECTURE.md) | Crate boundaries, IPC and the extension path |
| [Threat Model](THREAT_MODEL.md) | Assets, adversaries and mitigations |
| [Cache policy](CACHE_POLICY.md) | Retention and owner-use rules for cache payloads |
| [Framework artifact removal contract](FRAMEWORK_ARTIFACT_CONTRACT.md) | When generated framework output may be removed |
| [Scanner performance](SCANNER_PERFORMANCE.md) | Scan budgets and how performance is validated |
| [Budgeted scan continuation](SCAN_CONTINUATION_DESIGN.md) | Resuming scans that exceed their budget |
| [Privileged cleanup design](PRIVILEGED_CLEANUP_ADR.md) | Prerequisites for any administrator-level cleanup |
| [Design system](../DESIGN.md) | Visual contract and tokens |
| [Engineering guide](../AGENTS.md) | Workflow, version and safety rules for changes |

## Decision records

Each record states a dated decision and the scope it authorizes.

- [Permanent removal policy (#416)](PERMANENT_REMOVAL_416.md)
- [Cleanup operation scope (#362)](CLEANUP_362_SCOPE.md)
- [Scan coverage and accounting (#363)](CLEANUP_363_SCOPE.md)
- [Framework output review (#391)](CLEANUP_391_FRAMEWORK_CONTRACT.md)
- [DotSlash cache cleanup](DOTSLASH_CACHE_DECISION.md)
- [Homebrew cleanup operation boundary](HOMEBREW_CLEANUP_DECISION.md)
- [macOS maintenance decisions](MACOS_MAINTENANCE_DECISIONS.md)

## Audits and validation

Dated snapshots. They describe the state at the time they were written and are
not updated afterwards.

- [macOS cleanup coverage audit — 2026-09-26](CLEANUP_COVERAGE_AUDIT_2026-09-26.md)
- [Scan coverage follow-up — 2026-09-26](SCAN_COVERAGE_FOLLOWUP_2026-09-26.md)
- [Scan efficiency audit — 2026-09-27](SCAN_EFFICIENCY_AUDIT_2026-09-27.md)
- [GPU cache coverage — 2026-09-27](GPU_CACHE_COVERAGE_2026-09-27.md)
- [User-space cleaner coverage](USER_SPACE_CLEANER_COVERAGE.md)
- [Cleaner V2 validation sign-off](VALIDATION_SIGNOFF.md)
- [Windows folder and feature validation](WINDOWS_VALIDATION.md)

Supporting material lives in [`design/`](design) (per-issue design reviews and
brand assets), [`evidence/`](evidence) and [`validation/`](validation)
(screenshots and reports attached to specific versions).
