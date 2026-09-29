# Cleanup operation scope — issue #362

Implementation ledger for the 0.3.80 → 0.3.81 PR. The items below describe
available operations and deliberate scope limits, not a claim that every
observed byte is removable. Accounting and retained-scope navigation belong to
[#363](https://github.com/jaeyoung0509/neati/issues/363). Keep #362 open for the
privileged operation blocked by the owner's September 29 decision.

## Application and runtime operations

| Issue item | Implementation and limits |
| --- | --- |
| Editor offline assets | Reviewed depth-two CacheStorage units under Code, Code Insiders and Cursor's Service Worker/WebStorage scopes. Settings, IndexedDB, origin indexes and registrations remain. Editors and handles must be idle. |
| Cursor/Notion pending updates | Reviewed ZIP/DMG files, seven days inactive. The filename in `update-info.json` is retained and reread before mutation. Unknown manifest fields, unreadable files and active installers block the operation. |
| Named container scratch | Adds Podcasts `Data/tmp/StreamedMedia` to the existing geod/mediaanalysisd scopes. Three-day per-entry inactivity, process and handle guards apply. Downloaded episode libraries and other container temporary folders are outside scope. |
| Disposable database/index stores | The IDE adapter owns complete `index`, `caches` and `vcs-log` units under recognized versioned JetBrains/Android Studio cache directories. DB/WAL/SHM files move together. LocalHistory is excluded. This does not implement the IDE's entire Invalidate Caches action. |
| Codex runtime staging | Reviewed exact `codex-runtime-install-<six alphanumeric characters>` directories, recognized installer layout, seven-day inactivity, an independently activated format-2 macOS runtime, stopped owners and idle handles. Internal relative links move with the directory without dereferencing; escaping links are refused. Activated runtimes and `.previous` recovery copies are never targets. Unknown installer formats remain blocked. |
| Small benchmark residuals | Exact Google Updater `updater.log`/`updater.log.old` and Zsh `.zcompdump` variants have ordinary owner operations. Google updater preferences, versions and installation metadata remain. Project bytecode is excluded from default storage cleanup: the original benchmark included arbitrary project folders, which are not registered cleanup roots. No home/project-wide bytecode sweep is added. |

CloudKit stores, Safari website databases and general `Cache.db` files remain
protected: a cache-like filename does not establish that a database excludes
account, sync or offline state. Safari website-data deletion also affects site
state, so it is not represented as disposable cache cleanup. E5RT and
VisualIntelligence model stores retain the existing compiled-model protections.
No system daemon is stopped by any operation.

## Developer stores

| Issue item | Implementation and limits |
| --- | --- |
| Conda/mise compatibility | Verified set: Conda 26.5.3 and 26.7.2; mise 2026.9.15 and 2026.9.16. Actual commands ran in disposable homes. Support is constrained by the complete response schema and scope checks; this is not a claim that every intervening or future release has been tested. Existing timeout, executable replacement, inventory change and partial-result tests remain. |
| Gradle notifications | Exact versioned `release-features.rendered` files only, with typed Gradle process guards and exact handles. Gradle normally creates an empty marker: zero-byte markers are omitted, not advertised as disk savings. Removing a nonempty marker can show release highlights again. |
| IDE indexes | Complete regenerable index units as above; installed plugins, configuration, LocalHistory and old Application Support installations are preserved. |
| SwiftPM | Reviewed fixed `swift-package … purge-cache` operation. Complete preview includes `repositories`, `registry/downloads` and `manifests/manifest.db` below the standard macOS cache root. Command config/security/build paths are isolated in a disposable directory. Only the actually validated `Swift 6.4.0-dev` banner is accepted. Manifest WAL/SHM/journal companions block the command because the owner does not purge them together. Executable and recursive inventory identities plus every scope's open handles are rechecked. Artifacts, prebuilts, project `.build` trees and installed toolchains remain. |
| CocoaPods | Advisory downloaded store. A future `pod cache clean` adapter must preview the full command scope and preserve repositories/project Pods; generic removal is unavailable. |
| Android SDK | Advisory installation. SDK Manager owns package selection and uninstall; timestamps and `.temp` names do not authorize removal of installed SDK packages or system images. |
| OrbStack | Advisory exact known group-container data root, using bounded standard discovery and path checks. VM disks are not cache payloads. Supported Docker resource operations remain in Containers; machine removal needs a separate lifecycle adapter. No partial-component wildcard is introduced. |
| node-gyp/Electron | Complete versioned Node header SDK units and exact macOS Electron ZIP archives use ordinary Trash operations. Custom cache-root overrides are refused rather than guessed. Installed runtimes and apps remain. |
| Browser bundles/Corepack | Advisory executable distributions. Projects can depend on installed browser and package-manager versions for offline use. A versioned owner uninstall/cache command is required; no generic folder removal. Existing uv/Cargo ownership remains unchanged. |

## Additional actions

| Issue item | Implementation and limits |
| --- | --- |
| Home Trash | Separate Review Trash action, never preselected or added to cache candidates. A private, five-minute, one-shot snapshot authorizes only reviewed home-Trash entries. Native directory descriptors prevent ancestor/link traversal; the root is retained. Limits: 20,000 entries, depth 32 and 30 seconds per traversal/execution. Stop preserves remaining entries. Per-entry outcomes distinguish failures and removal; removed file data is not a free-space measurement. Other volumes' Trash is unsupported. |
| Privileged cleanup ADR | [Accepted design and distribution blocker](PRIVILEGED_CLEANUP_ADR.md). |
| System cache/diagnostic/log mutation | **Blocked, owner-approved.** No authenticated signed helper is shipped. `/Library/Caches`, DiagnosticReports, `/private/var/log`, DiagnosticPipeline and powerlog are not enabled through a shell fallback. Software Update and system databases remain protected. |
| Mail Downloads | Reviewed attachment copies in the two named Mail Downloads folders, after 30 days of whole-tree inactivity and idle Mail/handles. User edits in these copies are explicitly mentioned before confirmation. Mail message databases and original attachment stores are excluded. |
| Messages previews/stickers | Reviewed image files under the three named Mole preview/sticker-cache scopes. Only recognized image extensions within bounded traversal are offered. Databases, original `Messages/Attachments`, conversations and links remain. Messages/shared owners and handles must be idle. |
| Abandoned downloads | Reviewed direct `.download`, `.crdownload` and `.part` children of the platform's Downloads folder. Seven days is necessary but insufficient: browser/downloader owners and handles must also be idle at execution. Completed downloads remain. |
| Saved sessions and history | Saved Application State restores windows/unsaved workflow context; recent-item history is a user feature. They are outside default cleanup. A whole-home `.DS_Store` sweep would discard Finder folder presentation and cross unrelated project/content roots for negligible predictable benefit. None is a cleanup target. |

## Evidence

Reviewed September 29, 2026:

- [Electron session storage](https://www.electronjs.org/docs/latest/api/session)
  distinguishes CacheStorage from cookies, local storage and IndexedDB.
  [VS Code's webview worker](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/webview/browser/pre/service-worker.js)
  uses a regenerable resource cache. Scope follows these separate stores.
- [electron-updater's download helper](https://github.com/electron-userland/electron-builder/blob/master/packages/electron-updater/src/DownloadedUpdateHelper.ts)
  records the pending installer filename in `update-info.json`.
- [JetBrains cache invalidation](https://www.jetbrains.com/help/idea/invalidate-caches.html)
  and [IDE directory layout](https://intellij-support.jetbrains.com/hc/en-us/articles/206544519-Directories-used-by-the-IDE-to-store-settings-caches-plugins-and-logs)
  distinguish system caches from Local History and installed configuration.
- [Gradle's notification action](https://github.com/gradle/gradle/blob/master/platforms/core-runtime/gradle-cli/src/main/java/org/gradle/launcher/cli/WelcomeMessageAction.java)
  touches a versioned marker after showing release highlights.
- [SwiftPM's purge entry point](https://github.com/swiftlang/swift-package-manager/blob/main/Sources/CoreCommands/SwiftCommandState.swift)
  invokes repository, registry-download and manifest-cache managers.
  [ManifestLoader](https://github.com/swiftlang/swift-package-manager/blob/main/Sources/PackageLoading/ManifestLoader.swift)
  removes `manifest.db`; it does not authorize clearing all neighboring files.
  `validate_swiftpm_cleanup` exercises the installed Apple tool's actual behavior.
- [Chromium updater specification](https://chromium.googlesource.com/chromium/src/+/main/docs/updater/functional_spec.md)
  separates rotated logs from persistent updater preferences and installed versions.
- [node-gyp](https://github.com/nodejs/node-gyp),
  [Corepack](https://github.com/nodejs/corepack),
  [CocoaPods commands](https://guides.cocoapods.org/terminal/commands.html),
  [Android SDK Manager](https://developer.android.com/tools/sdkmanager)
  distinguish download stores from installed packages and owner operations.
- Installed Mole 1.55.0's `lib/clean/user.sh` names Mail Downloads and the three
  Messages preview/sticker scopes. `app_caches.sh` names Podcasts StreamedMedia.
  These path contracts informed independent adapters; no Mole implementation was copied.
- Installed ChatGPT 26.924.22138's runtime installer uses a temporary staging
  directory, validates its payload, renames `payload/codex-primary-runtime` into
  the independent activated root, and cleans staging in its finalizer. This
  establishes the supported format-2 staging contract; a prefix alone does not.
  Inspected `app.asar` SHA-256:
  `d0ba973179d2f717affd39e012b64a095464a54a51c6bccb7bc6b3d2a1cfba80`.
  No proprietary installer code is copied into this repository.

Actual commands, test results and visual evidence are recorded in
[the validation report](validation/cleanup-0.3.81/README.md).
