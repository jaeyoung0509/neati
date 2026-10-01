# Cache cleanup policy

## User-tool cache observations — #381

The catalog observes the exact standard `~/.oh-my-zsh/cache` and
`${XDG_CACHE_HOME}/oh-my-zsh` fallback namespaces, plus GitHub CLI's named
`${XDG_CACHE_HOME}/gh` local cache on macOS/Linux. XDG defaults to `~/.cache`
and uses the injected platform environment; invalid relative or traversing
overrides do not fall back to another namespace. These entries are advisory:
observed bytes never increase cleanable or selected bytes. File metadata is
measured without reading HTTP responses, shell scripts or configuration contents.

Oh My Zsh cache roots may contain generated/downloaded completion scripts,
update state and unknown files. Both known namespaces can be reported when
present; observation does not establish which one the current shell uses.
Custom `ZSH`/`ZSH_CACHE_DIR` installations and completion dumps outside these
namespaces are not resolved. GitHub CLI's `config clear-cache` removes its
entire effective cache; a verified owner adapter is required before this
operation is offered. No command, generic deletion or permission exception is
added. Configuration, credentials and extension installations are not searched.
An injected XDG root can alias another store, so the namespace's name alone
never grants authority or proves its contents disposable.

Fixed advisory roots are checked for linked ancestors before measurement.
Linked roots/ancestors and inaccessible locations remain explicit unavailable
observations; absent or verified empty locations are omitted. Existing depth,
cancellation, blacklist and no-follow traversal rules retain their accounting
qualifications. The vendor evidence, fixture coverage and remaining work are
recorded in [the #381 scope report](evidence/user-tool-discovery-381.md).

## Google Updater downloads and retained Mole scopes — 0.3.89

The exact current-user `~/Library/Application Support/Google/GoogleUpdater/crx_cache`
is one coupled owner unit. Its `metadata.json` and indexed downloads move together
to Trash. Installed versions, updater preferences and parent installation state
remain outside that unit. Fully measured idle units use zero-day retention and
ordinary Rebuild cleanup; no administrator command is used.

The supported index contains only `hashes`, with lowercase SHA-256 keys and an
`appid` string per record. Positive validation requires flat regular files,
current-user ownership, no group/world write bits, no executable bits,
no links, a CRX3 header and complete content hashes matching the index. Bounds
are 1 MiB of metadata, 128 archives, 2 GiB of payload and 20 seconds of payload
verification. These checks identify the disposable download-store format; they
do not validate CRX signatures or authorize arbitrary archives. Unknown layouts,
missing index entries and budget failures remain blocked with measured bytes.
Fresh GoogleUpdater owner and exact handle checks apply during scan, planning
and execution. Final validation checks every entry identity after the last use
probe before the whole-unit Trash move. Fixture tests never touch user caches.

OpenCode's `${XDG_CACHE_HOME}/opencode` is now observed alongside the legacy
cache/log roots. The inspected upstream package cache supplies executable plugins
and SDKs directly, including offline use. Its model JSON is a provider catalog,
not downloaded model weights. No installed-version purge contract was verified;
these bytes remain advisory and sessions, credentials and configuration stay out
of scope.

Code-signature clones are observed only at the two registered Chrome/IntelliJ
namespaces below the current user's OS-resolved `X` directory, derived from its
stated `C` cache root. They contain executable application snapshots with live
update/signature dependencies. No clone deletion is added. APFS shared blocks
are not resolved by inode accounting, so their observed footprint qualifies the
physical observed range rather than promising that amount of disk recovery.

Source contracts, the Mole accounting explanation and runtime limitations are
recorded in [the validation report](validation/cleanup-quick-panel-0.3.89/README.md).

## Developer temporary units — #369

The platform environment states user and shared temporary roots separately.
On macOS the shared root is `/private/tmp`; its `/tmp` alias is normalized for
the exact owner inventory. No operation scans or deletes the whole root.
The existing prefix observer remains advisory, including `neati-*` working
directories and Codex/browser/session scratch. It excludes only the exact
default `node-compile-cache` namespace, which the dedicated provider observes.

The first actionable contract is the actual Node **26.7.0 arm64** writer's
`v26.7.0-arm64-8d7ad2ee-<uid>` group under that exact namespace. Each group must
contain only bounded regular, non-executable, singly linked files with lowercase
eight-hex-digit names, the five-word Node header, matching payload length/CRC32
and the verified V8 cache marker. The cache root/group and payloads must belong
to the current user; writable-by-other-user directories/files are refused.
Validation uses held directory descriptors, no-follow file opens and metadata
stability checks. The newest modification/change timestamp across the complete
unit must be at least three days old. Fresh Node-owner and exact open-file
checks run at scan, planning and execution; the positive format and inactivity
contract is checked again after the final use probe and before the Trash move.

This is a narrow owner exception for regenerable compiled-cache payloads, not
permission to remove arbitrary compiled models, executables or project bytecode.
Unverified versions, architectures, flags/tags, layouts, source trees, recovery
artifacts, databases, locks and links remain advisory/blocked with observed
bytes where measurement succeeds. A private one-shot plan authorizes one
verified group; other groups, runtimes and source files remain. Trash movement
does not establish recovered free space. Windows has no adapter for this owner.

Evidence: [Node v26.7.0 module API](https://github.com/nodejs/node/blob/v26.7.0/doc/api/module.md#module-compile-cache),
[writer](https://github.com/nodejs/node/blob/v26.7.0/src/compile_cache.cc),
[header offsets](https://github.com/nodejs/node/blob/v26.7.0/src/compile_cache.h).
The committed binary fixture was generated by the installed Node v26.7.0 in a
disposable directory on September 30, 2026. Other writer formats are not claimed
compatible. Tests and actual real-user cleanup are distinct evidence.

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


### CocoaPods owner cleanup

CocoaPods download-cache removal is an explicit reviewed permanent owner action,
not ordinary cache-to-Trash deletion. The complete default
`~/Library/Caches/CocoaPods/Pods` root is previewed, including `Release`,
`External`, `Specs` and `VERSION`. CocoaPods adds `Pods` to its configured
cache directory; sibling entries in `CocoaPods` are not command targets.
Repositories, project `Pods`, configuration,
credentials and installed tools are outside its scope. Only CocoaPods 1.16.2
with a standard absolute RubyGems launcher is accepted. Unknown versions,
custom cache/home roots, unknown root entries, links, locks or busy/unknown
owners and handles block the operation. Blocked default stores remain visible
with observed bytes and cannot be preselected. Preview does not instantiate the
CocoaPods downloader cache, whose version reconciliation can delete an existing
root. Execution uses isolated configuration/repositories and disables plugins,
with fresh runtime and recursive candidate identities checked before mutation.
Standard user RubyGems installations resolve only the repository owning the
reviewed `~/.gem/ruby/<ABI>/bin/pod` launcher. The ABI must match its Ruby;
inherited gem/Bundler paths remain disabled. Loaded gem specifications are
included in the runtime identity alongside libraries and the interpreter.
See [the scope ledger](CLEANUP_362_SCOPE.md) for compatibility validation limits.
