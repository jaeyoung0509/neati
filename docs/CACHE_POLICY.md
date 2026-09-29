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

Owner processes are checked during discovery, planning and execution. On
macOS, user-cache and Application Support paths also carry their inferred
namespace owner into the private deletion plan. The process snapshot matches
bundle identifiers at a component boundary, bundle names and exact executable
names. A daemon namespace's last component can match its executable. This is
process evidence, not a general open-file proof. Unknown process state cannot
authorize cleanup. Flat executable containers under `/System/Library` with
no `Contents` or root `Info.plist` retain executable/name guards without
invalidating unrelated bundle identities. Missing or malformed metadata in
a conventional bundle still makes unmatched inferred owners unknown. Installed applications are not terminated automatically.

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

Developer-cache paths in this increment are literal `~/.cache` locations.
They do not claim to honor custom `XDG_CACHE_HOME` settings or search projects.
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

This batch delivers the payload and shared-guard increments. The linked issues
track these separately testable contracts:

- #357 (remaining contracts from closed #347/#348): proven cache-database
  adapters, open-handle validation, custom XDG roots and an owner operation for
  abandoned runtime staging. Their payload/catalog increments shipped in #355.
- #349: updater pending files, shared-container ownership and open-handle checks.
- #350: DeviceSupport retention by recency, Products/XCTestDevices contracts and
  simulator lifecycle operations.
- #351: Gradle argv-aware ownership, conda/mise owner commands and IDE stores.
  In particular, [`mise cache clear`](https://mise.jdx.dev/cache-behavior.html)
  also clears environment caches, so a single measured cache directory does
  not describe its complete mutation scope. Conda's operation must exclude
  `--packages`, whose symlink caveat is documented by
  [conda](https://docs.conda.io/projects/conda/en/stable/commands/clean.html).
- #352: explicit quit-and-clean/Trash operations and privileged adapters.
- #353: bounded observation of all include-list misses and excluded subtrees,
  including disjoint nested coverage and a dedicated retained-scope projection.

CLI #335 and usage diagnostics #354 are outside this batch.

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
depth-two CacheStorage units and preserves its whitelist. neati currently owns
a whole-store Trash operation, so copying that process policy alone would not
copy Mole's deletion unit. Fine-grained browser ownership remains #349/#352.
No log/temp retention thresholds changed in #359.
