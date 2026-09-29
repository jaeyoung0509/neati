# Cache cleanup policy

## Default behavior

A cache namespace is discovered separately from its eligibility for deletion.
Regenerable payloads under `~/Library/Caches` use a zero-day age threshold,
including Apple namespaces. A bundle prefix alone is not a reason to retain
ordinary payloads. Fully measured, idle Safe and Rebuild entries are selected
by default; the user starts cleanup explicitly. No additional ordinary-cache
review dialog is required.

The cleanup estimate includes only entries that the execution policy can
remove. Databases and their companions, locks, credentials, settings,
executables, application bundles and E5RT compiled-model stores stay protected.
For stale-content cleanup, a protected directory's entire descendant tree is
excluded from the reclaim estimate while its measured bytes remain observed.
A whole-directory cleanup cannot bypass these classifications.

Owner use is checked during discovery, planning and execution. On macOS,
ordinary pruned payloads under `Library/Caches` and `Library/Logs` use a bounded
open-file check on the exact unit. A running app or daemon alone does not block
all its disposable files. Explicit catalog executable guards still take
precedence, and other stores retain inferred namespace-owner guards. Unknown
process or handle evidence cannot authorize cleanup. Structured-state and
compiled-model exclusions apply independently of these checks.

An explicit “Review apps to quit” action is available for verified user apps
whose caches genuinely require owner exit. The backend maps current scan item
IDs to existing short-lived process leases; protected/system processes remain
unavailable. The user saves work and confirms a graceful quit request. Cancellation
stops the next step, and no automatic force quit is offered. A fresh scan and
new cleanup review are required before deletion. Windows currently asks the user
to quit the app themselves because its process adapter has no graceful operation.

## Scope and accounting

| Location | Behavior |
| --- | --- |
| Apple and third-party user caches | Idle, disposable payloads are eligible immediately |
| Help, Xcode, browser and package-manager caches | Keep their dedicated signature or provider |
| CloudKit, Safari state, wallpaper, VisualIntelligence and E5RT stores | Observed or protected; no generic whole-store deletion |
| `~/.cache/{typescript,vite,webpack,eslint,prettier}` | Exact literal directories; payload cleanup when Node/Bun/tool processes are idle |
| `~/.cache/codex-runtimes` | Installed runtime observation; zero cleanable bytes |
| Zed `node/cache` and `node/*/cache` | Download payloads only; installed runtimes/extensions/language servers remain outside scope |
| Xcode DeviceSupport | Existing 30-day retention plus active Xcode/Simulator guards |

Named JavaScript cache paths honor the injected `XDG_CACHE_HOME`, defaulting
to `~/.cache`. Gradle roots honor `GRADLE_USER_HOME`, defaulting to `~/.gradle`.
Invalid relative or ambiguous overrides do not fall back to another location.
No project search is introduced.
Running Node/Bun blocks these generic JavaScript cache signatures because the
process executable alone cannot identify which build tool is using them.

Signature exclusions, direct-child include/exclude prefixes and selector
scope are checked again when authorizing a plan. An excluded path cannot gain
permission from a forged selection. Dedicated roots are excluded from broad
rules so their bytes and authority are not duplicated.

Observed, cleanable and selected bytes are different populations. A runtime
store's observed size is not a prune estimate. Existing Storage rows show
retained reasons; an incomplete observation remains a lower bound or unknown.

## Evidence

Reviewed against neati 0.3.76 and Mole commit
`f32fa0c6b082ed76705cbc45f2c42169a09e2503`:

- [Apple's Library directory contract](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/MacOSXDirectories/MacOSXDirectories.html)
  distinguishes regenerable Caches from Application Support data.
- [Mole user-cache cleanup](https://github.com/tw93/Mole/blob/f32fa0c6b082ed76705cbc45f2c42169a09e2503/lib/clean/user.sh#L156-L225)
  combines broad discovery with live-owner checks.
- [Mole compiled-model protection](https://github.com/tw93/Mole/blob/f32fa0c6b082ed76705cbc45f2c42169a09e2503/lib/core/app_protection.sh#L306-L327)
  preserves E5RT stores. Its runtime failure rationale is an upstream report,
  not a reproduced neati experiment.
- [Mole Zed paths](https://github.com/tw93/Mole/blob/f32fa0c6b082ed76705cbc45f2c42169a09e2503/lib/clean/app_caches.sh#L438-L453)
  identify Node download caches separately from installed editor content.

Mole is a source of resource contracts and fixtures. Its shell fallbacks and
error suppression are not a deletion authorization mechanism for neati.

## Remaining issue work

The consolidated operation tracker is #362; accounting, retained-scope navigation,
deduplication and observation performance are #363. They replace #349, #351,
#352 and #353. CLI #335 and Xcode #350 stay separate. Usage diagnostics #354 was
closed after the owner's verification.

[The #362 scope ledger](CLEANUP_362_SCOPE.md) lists each implemented operation,
unsupported owner store and the approved privileged-cleanup blocker. Ordinary
scope decisions must not be described as implementations of unsupported commands.

## Mole-aligned disposable download defaults

Ready Homebrew download files and Cargo registry `.crate` archive units are
selected by default and use ordinary explicit cleanup, including Quick Clean.
Their owner adapters still validate scope, file/archive shape, process state,
identity and the current inventory before mutation. A provider implementation
alone is not a reason to require a second confirmation for disposable downloads.
Cargo extracted sources/git stores and Homebrew API metadata remain outside
this authorization. Homebrew installed-version cleanup and browser offline
stores retain their distinct reviewed contracts.

The installed Mole 1.55.0 `lib/clean/dev.sh` processes Cargo registry archive
caches in its Rust cleanup and Homebrew downloads in normal cleanup.
`lib/clean/user.sh::clean_browsers` guards ordinary Chrome profile caches but
calls `clean_service_worker_cache` separately; `lib/clean/caches.sh` enumerates
depth-two CacheStorage units and preserves its whitelist. Before this batch, neati owned
a whole-store Trash operation, so copying that process policy alone would not
copy Mole's deletion unit. This batch uses depth-two cache directories; see the
contract below.
No log/temp retention thresholds changed in #359.

## Mole comparison batch: browser units, developer stores and retained bytes

- Chromium offline caches use disjoint depth-two directory units. Origin
  indexes are separate advisory observations. Each selected unit goes to Trash;
  the plan and result state that mutation channel correctly. On macOS, each
  offline unit is checked for open handles during scan, planning, and immediately
  before the Trash move. Chrome may remain running when the selected unit is idle.
  In-use units remain reviewable and unknown use remains blocked. Component
  update stores retain their browser-wide owner guard. Windows also retains
  the offline browser guard until an exact-use adapter is implemented.
  Offline-asset removal still requires review of its network/re-download effect.
- Container and group-container payloads and ordinary application logs use
  zero-day retention. macOS additionally probes open files within each unit
  with a bounded `lsof` invocation. A warning, timeout or unreadable result
  cannot establish an idle unit. Generic structured-state protections remain.
- Gradle local build-cache payloads are separate from advisory dependency
  stores. Seven-day daemon logs and three-day worker scratch payloads have
  their own units. Known Gradle Java launcher/worker arguments guard all three;
  an unrelated Java application does not. Missing Java arguments remain unknown.
- JetBrains and Android Studio disposable payloads have explicit IDE owners.
  Local history, configuration, plugins, databases and locks are retained.
- Conda uses only `clean --yes --index-cache --tarballs --logfiles --json`,
  preceded by its JSON dry-run. `--all`, `--packages`, `--force-pkgs-dirs` and
  arbitrary user arguments are not accepted. mise uses `cache clear`; its
  diagnostic JSON must describe cache, external task-cache and state roots,
  including environment caches. Scope overlapping installed tools, configuration
  or known user-content roots is refused. Both are reviewed macOS owner actions
  tied to a candidate digest and executable identity, with bounded commands,
  repeated discovery, post-action observation and no filesystem fallback.
  Their platform adapters are exercised with temporary executable fixtures.
  Neither tool was installed for the live benchmark; real installed CLI
  compatibility is not claimed by those fixture tests.
- Named geod/mediaanalysisd container temporary payloads use three-day inactivity.
  Completed Brave crash uploads use seven days, diagnostic/crash reports fourteen.
  Suggestions and Help metadata remain observation-only. These entries explain
  measured Mole gaps without treating every reference row as a deletion grant.
- Storage expands a compact retained-byte breakdown from backend eligibility,
  quality and size facts. Each non-candidate byte is counted once per retained
  observation. Existing overlap ranges remain visible; reason totals are labeled
  upper bounds when observations overlap. Partial results explicitly exclude
  unknown bytes. No frontend message matching decides eligibility.

The remaining work from this historical increment is consolidated in #362
(operations) and #363 (coverage and accounting). See the current scope ledger
above; the original trackers are no longer active.

Owner command references reviewed September 29, 2026:
[Conda clean](https://docs.conda.io/projects/conda/en/stable/commands/clean.html),
[mise cache behavior](https://mise.jdx.dev/cache-behavior.html),
[Gradle directories](https://docs.gradle.org/current/userguide/directory_layout.html),
[JetBrains system directories](https://www.jetbrains.com/help/idea/tuning-the-ide.html).

## Actionable cleanup batch

The headline and category metrics show **ready now** bytes. Review-required and
running-owner candidates appear separately; neither is presented as immediately
executable or as freed disk space. The selection footer continues to show the
actual selection. Moving a unit to Trash is reported separately from permanent
delete and does not claim that free disk space increased.

mise doctor may omit default settings. Its command adapter accepts an empty
settings object, retains strict root validation, and disables update checks,
automatic updates and opportunistic cache pruning in its own command environment.
This prevents the verification command from recreating `latest-version` or
pruning unrelated cache entries. Tool commands still rebind executable and
candidate identities at each mutation boundary.

## Reviewed owner actions — 0.3.81

The new owner adapters and their explicit limits are listed in
[CLEANUP_362_SCOPE.md](CLEANUP_362_SCOPE.md). Editor offline assets, updater staging,
IDE indexes, runtime installation staging, Mail copies, Messages previews and
incomplete downloads require review. Node header downloads, Electron ZIPs, Zsh
completion dumps, Gradle markers and updater logs use ordinary cleanup when
verified idle. Every operation re-derives its native scope and use state.

Home Trash emptying is a separate permanent action with an exact expiring
snapshot and per-entry outcomes. It never joins the cache candidate selection.
Mail Downloads and incomplete Downloads are typed reviewed scopes, analogous to
the existing reviewed storage workflows; neither broadens generic filesystem
cleanup or the blacklist. Private plans remain in the backend.
