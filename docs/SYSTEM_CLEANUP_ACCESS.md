# System cleanup access and retained state — #362

This assessment records a read-only probe on **October 1, 2026**, macOS
**27.0.1 (26A434)**, source **0.3.100 / 00ca207f**, effective uid **501**.
It establishes access and owner/state facts, not successful cleanup or native
TCC verification. No system cleanup adapter, daemon command, elevation,
permission change or real-user deletion was run.

Full Disk Access, filesystem ownership/modes/ACLs and SIP are separate gates.
Readable or writable bytes are not necessarily disposable. The private planner
and fresh execution guards remain authoritative. Paid signing and privileged
helper mutation remain owner-deferred under the
[privileged-cleanup ADR](PRIVILEGED_CLEANUP_ADR.md).

## Per-operation decision

| Scope | Owner, lifecycle and retention | Current access / privacy limits | neati operation |
| --- | --- | --- | --- |
| `~/Library/Logs/DiagnosticReports`, `~/Library/Logs/CrashReporter` | The installed `ReportCrash(8)` distinguishes user-agent reports from system-daemon reports and describes per-process oldest-report pruning. It specifies no portable day threshold. neati's existing user signature applies **14 days per entry**, with ReportCrash/diagnosticd guards. | Current-user scope; actual scan, plan and final exact-use checks may still fail. A grant does not bypass ownership or active writers. | Existing stale-content cleanup only for positively measured eligible entries. Keep recent reports and structured state. The 14-day rule is neati policy, not Apple's default. |
| `/Library/Caches` | Mixed OS/third-party namespaces, not one cache owner. Seven direct entries were seen; none proved current-user-owned and three denied metadata access. No complete disposable namespace was established. | Root is `uid 0`, sticky `1777`, readable and writable to this shell. Sticky-directory rules and each entry's ownership remain independent. EPERM does not identify its cause or prove TCC. | Observation only. Never remove the root or reinterpret root writability as entry authority. A future namespace needs its own positive owner/format/retention/use contract. |
| `/Library/Logs/DiagnosticReports` | `ReportCrash(8)` assigns system-wide reports to the daemon rather than the user agent. All **80** observed entries were `uid 0` (78 regular files, two directories), preserving system troubleshooting evidence. | Root `0:250 / 0770`; root and observed entries are writable to this shell. That does not satisfy neati's current-user ownership invariant or establish a system-report retention adapter. | Retain system reports. Never inherit the user-report signature or claim that all system reports require elevation solely from their root's mode. No administrator fallback. |
| `/private/var/log/asl` | `aslmanager(8)` describes syslogd-owned rotation, archiving and expiry. Its documented default seven-day TTL is configurable per store/module and message, not a blanket rule for all log files. | Root `uid 0 / 0755`; 12 direct entries are root-owned; shell can read/search but cannot write/search. Native privacy access is separate. | Advisory. Use OS-managed retention; do not run an owner command, stop syslogd or delete its database/files generically. |
| `/private/var/log/DiagnosticMessages` | Apple's Console guide identifies the MessageTracerStore as Mac analytics reports. Eight observed files are root-owned; no independent current-user discard unit was established. | Root `0:80 / 0750`; read/search succeeds, write/search fails. | Retain/advisory. No generic log sweep or inherited ASL seven-day rule. |
| Other `/private/var/log` entries | Multiple writers and rotation policies. The installed ASL/log manuals distinguish configured legacy stores and the unified datastore. | Root `uid 0 / 0755`; read/search succeeds, write/search fails; 38 of 40 direct entries are root-owned and none current-user-owned. | No whole-root or age-only operation. Observation never becomes permission to remove another writer's state. |
| `/private/var/db/diagnostics` | Unified logging state. Installed `log(1)` describes `erase` as deleting the main and in-flight datastore; `--all` additionally affects TTL/fault/error stores. This is a broad stateful owner operation, not a cache-unit prune. `diagnosticd(8)` describes a live bridge from logd to Console/log stream. | Root `0:80 / 0750`; all 14 direct entries root-owned; write/search fails. | Retain. Do not invoke `log erase`, infer that the daemon owns every diagnostic path, or expose this global operation as cache cleanup. |
| `/private/var/db/DiagnosticPipeline` | The actual directory contains three SQLite stores with their matching WAL/SHM companions plus Configuration/Logs directories. This proves coupled structured state. The installed diagnostic daemon manuals do **not** establish its exact owner or a disposable retention unit. | Root `uid 0 / 0755`; all 11 direct entries root-owned; write/search fails. | Protect database, companions and configuration. No independent lifecycle contract justifies removal, even if access later changes. Unknown owner/retention is explicit, not a zero-byte estimate. |
| `/private/var/db/powerlog` | Installed `powerlogHelperd(8)` links the launchd-owned service to Battery Usage UI and says not to run it manually. It supplies no disposable-store retention contract. The bounded probe sees a root-owned Library child; nested contents were not measured. | Root `uid 0 / 0755`; read/search succeeds, write/search fails. | Preserve power state and writers. Zero allocated bytes for the **directory entry** is not a zero-byte tree. No daemon invocation or database deletion. |

The installed manual pages are primary evidence for this OS image; older manual
revision dates are not proof that every documented implementation detail is
current on every macOS version. No fixed expiry is invented for DiagnosticPipeline,
powerlog, system crash reports or arbitrary cache namespaces.

## CloudKit, Safari and index state

| Namespace / state | Positive owner and lifecycle evidence | Enforcement / remaining limit |
| --- | --- | --- |
| `CloudKit`, `com.apple.CloudKit` | Apple's `CKSyncEngine.State` includes server change tokens, subscriptions, pending changes and the current account; the state/update APIs require persistence across launches. This is evidence of synchronization state, **not proof that every byte under every CloudKit path is indispensable**. No complete disposable subunit is established here. | Existing user-cache exclusions also apply to container/group-container caches. Direct prefixes and nested bare names are protected case-insensitively on macOS. Observed bytes remain visible; cleanable/selected bytes stay zero. Fresh recursive checks retain newly appeared protected entries. |
| `com.apple.Safari` / Safari website stores | Apple's Safari UI removes cookies and website data with documented sign-out/behavior consequences. WebKit distinguishes HTTP cache from Service Worker/Cache API/IndexedDB/offline and persistent origin state; eviction is an owner policy, not an arbitrary-folder permission. A WKWebView's API controls **its configured datastore**, not another app's Safari profile. | Retain the registered Safari state namespaces, including container/group-container names and nested state. No external Safari path is authorized through another app's `WKWebsiteDataStore`. Existing reviewed Chromium cache units keep their separate contracts. |
| Indexes, databases, WAL/SHM, configuration and locks | `URLCache` has a receiver-specific, configurable disk cache and read/write lifecycle; a file named `Cache.db` alone does not identify that owner or grant a disposable-store contract. SQLite companions and recognized store markers are coupled structured state; an arbitrary file named `index` is not automatically classified. An age or filename cannot prove there are no pending/offline dependencies. | Shared structured-state classification protects recognized database/companion/configuration/lock shapes and excludes their descendants from estimates. A generic `index` filename alone is not a classification or authorization rule. Registered Help plist indexes and Siri learned state remain protected/advisory; verified JetBrains IDE index units keep their independent owner contract and preserve LocalHistory. Verified GoogleUpdater/Corepack/browser units likewise retain their reviewed whole-unit contracts. No generic SQLite exception or helper operation is added. |

The CloudKit/Safari protection is a conservative inference from owner/state
contracts and the absence of a positively identified disposable unit. It is not
a claim that the framework APIs reveal the contents of a particular user's files.
Tests use temporary fixtures with payloads, mixed-case direct/nested protected
names, forged selected rows and a protected namespace created after plan validation.
They verify observed/cleanable separation and retention during execution; no real
CloudKit or browser data is read or removed by those tests.

## Bounded access evidence

The probe used `lstat` and `access(R_OK|X_OK / W_OK|X_OK)` on these exact roots
and at most **128 direct entries per root**. It read no file contents, followed
no links, did not recurse and did not test deletion. The shell process's access
is not evidence for the unsigned application bundle's TCC grant.

| Root | uid:gid / mode | Read/search | Write/search | Direct entries / denied metadata | Direct-entry allocated bytes |
| --- | --- | --- | --- | --- | --- |
| `/Library/Caches` | `0:80 / 1777` | yes | yes | 7 / 3 | 0; directories/unknown descendants, **not a zero tree** |
| `/Library/Logs/DiagnosticReports` | `0:250 / 0770` | yes | yes | 80 / 0 | 36,433,920 |
| `/private/var/log` | `0:0 / 0755` | yes | no | 40 / 0 | 46,469,120 |
| `/private/var/log/asl` | `0:0 / 0755` | yes | no | 12 / 0 | 1,036,288 |
| `/private/var/log/DiagnosticMessages` | `0:80 / 0750` | yes | no | 8 / 0 | 9,228,288 |
| `/private/var/db/diagnostics` | `0:80 / 0750` | yes | no | 14 / 0 | 10,649,600 |
| `/private/var/db/DiagnosticPipeline` | `0:0 / 0755` | yes | no | 11 / 0 | 4,751,360 |
| `/private/var/db/powerlog` | `0:0 / 0755` | yes | no | 1 / 0 | 0; nested Library tree unmeasured |

Do not sum this column: parent/child roots overlap, and these are direct-entry
metadata totals rather than recursively measured, uniquely reclaimable bytes.
Raw local metadata is retained privately because report filenames can identify
user apps/machine names. Its SHA-256 is
`3bba886e1c2da02be822a75f41d8c0000c578834de752e63648c7c11e535825b`.
Only aggregate, de-identified facts appear here.

## Permission setup and native evidence

Settings provides access instructions before scanning; typed privacy gaps link
to the same flow in Storage. Returning requests one fresh scan after ongoing
work completes; Check Access is the fallback when focus events are unavailable
or macOS requires relaunch. Denied/revoked access leaves unknown locations while
other fully verified units remain usable. Full Disk Access does not imply an
administrator role or deletion eligibility.

The **unsigned native** grant/deny/revoke/relaunch/update cycle remains a separate
acceptance item in #362. Browser fixtures, shell access probes and automated
recovery tests do not establish actual TCC behavior or grant persistence across
bundle replacement. This report does not mark that native item complete.

Primary references:

- [Apple Console report types](https://support.apple.com/guide/console/reports-cnsl664be99a/1.1/mac/26)
- [CloudKit sync engine state](https://developer.apple.com/documentation/cloudkit/cksyncenginestate?language=objc) and [state-update persistence](https://developer.apple.com/documentation/cloudkit/cksyncenginestateupdateevent)
- [Safari website data](https://support.apple.com/guide/safari/manage-cookies-sfri11471/mac)
- [WebKit storage policy](https://webkit.org/blog/14403/updates-to-storage-policy/)
- [WKWebsiteDataStore](https://developer.apple.com/documentation/webkit/wkwebsitedatastore?changes=la_5&language=objc)
- [URLCache](https://developer.apple.com/documentation/foundation/urlcache?changes=_3) and [receiver cache removal](https://developer.apple.com/documentation/foundation/urlcache/removeallcachedresponses%28%29)
- [Apple filesystem access layers](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox)
- Installed `ReportCrash(8)`, `diagnosticd(8)`, `diagnosticextensionsd(8)`, `powerlogHelperd(8)`, `aslmanager(8)`, `asl(3)` and `log(1)` on the recorded macOS build. No privileged commands from those manuals were executed.
