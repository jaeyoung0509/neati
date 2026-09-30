# System cleanup access assessment

Issue #362, owner decision September 30, 2026. This is a contract assessment,
not a record of successful deletion or a claim about every machine's modes.
No new system cleanup adapter is enabled by the permission-setup PR.

Full Disk Access addresses macOS privacy restrictions. Ownership, POSIX modes,
ACLs and SIP remain separate. A readable path or a successful access probe does
not establish deletion permission or disposable lifecycle semantics. The
existing trusted planner and final execution guards remain authoritative.

| Scope | Existing owner/lifecycle evidence | Privacy and filesystem access | Current mutation decision |
| --- | --- | --- | --- |
| `~/Library/Logs/DiagnosticReports`, `~/Library/Logs/CrashReporter` | Registered `system.crash_reports`: per-entry 14-day retention; ReportCrash/diagnosticd activity guard | Current user's locations; actual scan/plan/execution can still fail on access or use checks | Existing stale-content operation remains available only for verified eligible entries. Recent reports are retained. |
| `/Library/Caches` | Mixed system and third-party namespaces; the root is not one disposable store | Modes and owner differ per namespace. Full Disk Access does not change them | Observation-only catalog entry. A future namespace needs exact owner/lifecycle evidence and a reviewed typed adapter even if writable by the current user. No whole-root removal. |
| `/Library/Logs/DiagnosticReports` | System-wide reports, separate from the current user's registered report roots | System ownership/ACLs may require elevation; privacy grants do not establish write access | No adapter. Retain troubleshooting data; a separate retention, active-writer, scope and platform test contract is required. Never inherit the user-report authorization. |
| `/private/var/log/asl`, `/private/var/log/DiagnosticMessages` and other `/private/var/log` entries | Legacy/current system logging stores vary by OS; some are structured stores | System-managed roots and writers; user privacy access is insufficient evidence | Existing named legacy roots remain advisory. No generic log sweep, daemon stop or database removal. A real owner contract is still missing. |
| DiagnosticPipeline | OS-managed diagnostic infrastructure; no proven disposable unit/retention adapter in neati | Location and policy vary by OS; no validated mutation-access claim | Unsupported pending separate lifecycle and platform evidence. Discovery or a cache-like name cannot authorize it. |
| powerlog | OS-managed power diagnostics; may include structured data | No validated current-user mutation adapter | Unsupported. Preserve databases and active writers. Full Disk Access is not a cleanup adapter. |

The assessment does not propose changing file ownership, installing a helper,
invoking sudo, relaxing SIP or adding blacklist exceptions. Paid signing and
privileged mutation remain owner-deferred. A future user-accessible namespace
must pass a separate review of its exact catalog/owner contract; permissions
alone do not resolve these lifecycle gaps.

## Permission setup and evidence limits

Settings provides access instructions before a scan; typed privacy gaps offer
the same instructions in Storage. Returning from the explicitly opened Settings
flow requests one fresh scan. Ongoing cleanup/scanning completes first. Check
Access is the fallback when focus events are unavailable, and after a required
macOS relaunch. A denied or revoked grant leaves locations unknown; other
verified items remain usable. The platform copy states privacy access as a
possible explanation, because a protected path can also fail on file modes.

The unsigned native bundle's grant/deny/revoke/relaunch/update sequence requires
manual OS evidence and remains open in #362. Automated recovery tests and
browser screenshots do not validate macOS TCC or establish grant persistence
across app replacement. No system data or real user cache is modified by tests.

References: [Apple file-access controls](https://support.apple.com/en-ca/guide/security/secddd1d86a6/web),
[Apple filesystem access layers](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox),
[Apple file permissions](https://support.apple.com/guide/mac-help/change-permissions-for-files-folders-or-disks-mchlp1203/mac).
