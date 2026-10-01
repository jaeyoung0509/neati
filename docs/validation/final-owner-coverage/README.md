# Owner coverage follow-up — October 1, 2026

The patch binds Arc's verified root/runtime, expands exact renderer-cache owner
units, adds Corepack's reviewed complete v1 command and records one additional
CocoaPods/runtime tuple. Generic cleanup cannot bypass a blocked Arc owner, and
renderer source/runtime files and inconsistent Corepack version markers are
refused. Sections below separate implemented operations from evidence-backed
protected decisions.

This is the incomplete validation checkpoint for [#362](https://github.com/jaeyoung0509/neati/issues/362)
and [#382](https://github.com/jaeyoung0509/neati/issues/382). Recorded public-
distribution/fixture runs used develop `00ca207` (0.3.100) on macOS 27.0.1
(26A434), arm64. Final synchronized version and integration checks remain in
this PR. Native grant/deny/revoke/relaunch/replacement evidence is not supplied
by shell access probes or fixture process ports.

## Android SDK: resumable work and installed packages remain protected

The pinned AOSP `platform/tools/base` revision
`974c89e1adb23eb495a725d83602b3f8e747ad9e` has
[`AbstractPackageOperation`](https://android.googlesource.com/platform/tools/base/+/974c89e1adb23eb495a725d83602b3f8e747ad9e/repository/src/main/java/com/android/repository/impl/installer/AbstractPackageOperation.java)
handling `.temp/PackageOperationNN`, `.downloadIntermediates`,
`.prepareComplete`, and installed-package `.installData` restart references.
Preparing a package does not mean the operation has completed; a later run can
resume from its recorded temporary path. Its own orphan cleanup resolves all
package installation references before pruning unreferenced numbered staging
folders. It is therefore a lifecycle decision, rather than an age-only rule.

neati does not establish a complete SDK-wide reference inventory, installed
command distribution identity or an idle transaction/locking contract for
these records. No standalone fixed-argument staged-cache command with a complete
read-only preview was verified. Consequently, `.temp` or an old timestamp does
not grant cleanup authority. Installed build tools/platforms/system images,
resumable staging and download intermediates remain observed/protected through
the existing registered SDK observation; no SDK process is stopped.

[SDK Manager](https://developer.android.com/tools/sdkmanager) owns explicit
package installation and removal. Removing a system image or installed package
can affect an AVD and a project's selected tools, so it is a package-selection
operation with different prerequisites from clearing a disposable download.
No package uninstall is exposed by generic cache selection. This decision rests
on the recorded owner lifecycle, not on whether sdkmanager happened to be on PATH.

The saved pinned source SHA-256 is
`49724576bd7d70a76c7e478d3b4074362c0d3a3da43b81b76f0782373c4fb246`.
The earlier unpinned source snapshot differs; the pinned source was separately
saved and the relevant lifecycle and orphan-reference behavior rechecked.

## OrbStack: use Containers for Docker, preserve VM state

The installed public bundle is OrbStack 2.2.3, CFBundleVersion 20963,
`dev.kdrag0n.MacVirt`. Its CLI reports 2.2.3 (2020300), commit
`c83556b0ef8f1ba9a33abbb194622b6b7a1c0307`. Bundle and CLI build identifiers
are separate facts. Only public Info.plist, `version`, `delete --help` and
`reset --help` were inspected; no machine/container operation was performed.

The actual `delete` help says that the selected machine is stopped and its files
are permanently lost. `reset` covers **all Linux machines and Docker data**.
Neither operation is a disposable download-cache command. The exact group
container `HUAQ24HBR6.dev.orbstack/data` therefore remains advisory, and neati
never removes its disk image, machine store or databases with a filesystem
fallback. This matches the owner's [machine commands](https://docs.orbstack.dev/machines/commands).

OrbStack's [Docker engine](https://docs.orbstack.dev/docker/) is the established
owner for Docker objects. Existing Containers operations retain their existing
CLI/daemon, resource and confirmation contracts for image/build-cache/stopped-
container pruning and separately reviewed volumes. They act on the responding
Docker-compatible runtime/context; they do not imply a direct operation on the
OrbStack group-container directory or Linux machines. No `orb reset`, `orb delete`,
global system prune, generic PID termination or implicit VM shutdown is added.

## Browser distributions: executable dependencies stay advisory

Public npm distributions were inspected without launching a browser or touching
a real profile: `@puppeteer/browsers` 3.2.3 and `playwright-core` 1.63.0.
Their tarball SHA-256 values are respectively
`c0867d72d988b2aae8bb1f19cf39f558833be7996b8dc790e9188701a0a31161` and
`208593d4e1bcd8f8fe5f869cad1cc332dc7f1d70dc1d58c102dc3ac36e30f26c`.

Puppeteer's owner cache tracks browser/platform/build IDs, alias mappings and
executable paths in metadata. Targeted `uninstall` updates that metadata and
removes the selected installed browser; `clear` recursively removes the entire
cache root. Browser bytes are executable/offline test dependencies, not HTTP cache.
[The owner uninstall API](https://pptr.dev/browsers-api/browsers.uninstall) names
that distinct operation.

Playwright's `uninstall` changes `.links` package-reference registration and runs
its install-cache reference traversal. The all variant unlinks every package
registration. Stale browser removal uses the remaining reference graph and
`INSTALLATION_COMPLETE` markers, rather than file age. A project can require the
pinned revision again for offline tests; known links also do not prove absence of
custom executable users. The [owner browser lifecycle](https://playwright.dev/docs/browsers#uninstall-browsers)
is not a complete bounded neati preview/use/dependency contract.

No reviewed private-plan adapter currently binds these installed executable
units, every affected metadata mutation, current project/reference state and
fresh process/handle state. Known installed distribution roots therefore remain
visible/advisory with cleanup unavailable. There is no executable exception in
generic cleanup, arbitrary whole-profile deletion or automatic browser quit.
This is an explicit scope and offline-dependency limitation, not a missing-tool
finding. Renderer/offline/component cache units have separate contracts.

## Corepack: reviewed complete v1 owner command, with offline consequence

The public Corepack 0.36.0 npm tarball SHA-256 is
`9128cfe26aee0c99f4fc68c15c6b8b12309a68be0de2c012cdfea2a463cb722f`.
The adapter pins both shipped corepack.js and corepack.cjs bytes, binds an
installed Node 26.7.0 runtime, and authorizes only the complete default
`~/.cache/node/corepack/v1` store. Its fixed owner command is `cache clean`;
there is no filesystem fallback. `lastKnownGood.json`, installed shims,
configuration, credentials and projects remain outside its scope. Empty,
relative/custom COREPACK_HOME/XDG roots and unknown distributions are unavailable,
rather than silently routed to the default path.

The owner store contains executable package-manager distributions used by
projects. It receives explicit review and the consequence that offline pinned
projects can fail until those managers are downloaded again. It is not counted
as ordinary cache Trash, and the adapter reports permanent removal separately
from disk free-space change. Scan, prepare and final execution bind the complete
bounded inventory, ownership, safe ancestry, package-manager markers/binary maps,
runtime identity and fresh owner/handle verdicts. Version markers must bind their
stable numeric X.Y.Z directory and SHA-512 hash, including any hash suffix;
prerelease/URL/mismatched formats are unavailable for this recorded contract.

The actual task-owned Corepack-populated pnpm 10.17.1 JavaScript distribution
could run with Corepack networking disabled before cleanup. The owner command removed
20,451,328 allocated fixture bytes; the same offline command then failed with
network access disabled. `.corepack`, runtime/project/shim/configuration and
lastKnownGood sentinels were verified. Five further actual command fixtures
blocked final ancestry replacement, same-byte marker replacement with restored
mtime, runtime-content change, busy and unknown handles; each removed zero bytes.
The process/handle port in this harness was a fixture, so this does not claim a
native lsof/real-project run.

A newer pnpm binary package can perform its own cold bootstrap download even
when Corepack networking is disabled; the offline assertion deliberately uses
the observed JavaScript distribution and is not generalized to every manager.
The adapter's mutation scope remains the same complete v1 owner store.

## CocoaPods: one additional actual env-launcher/runtime tuple

Existing CocoaPods 1.16.2/system Ruby 2.6.10 evidence remains supported. The new
actual task-owned run used CocoaPods 1.17.0 with Homebrew 7.0.7's portable Ruby
4.0.7 (Ruby ABI 4.0.0), the standard RubyGems user repository and its env-shebang
launcher. The runtime/distribution identity and exact version tuple are bound;
unknown ABI/version tuples and arbitrary custom gem homes are refused.

The complete Pods-child owner scope includes Release/External/Specs/VERSION,
not the parent CocoaPods cache root. The actual isolated owner operation removed
20,480 allocated fixture bytes and preserved parent siblings, projects,
repositories/settings and installed gems. Ruby/configuration/plugin environment
is isolated before launch. This is a recorded exact tuple, not compatibility
with every Homebrew, env wrapper, Ruby installation or future CocoaPods release.

## Browser layout and native acceptance follow-up

Arc public bundle 1.166.0/build 87668 with ArcCore Chromium 154.0.8037.58 establishes
`Arc/User Data` using static public initialization/disassembly evidence. That
exact tuple is required for actionable Arc cache units; `Arc` without User Data
and bundle-ID-spelled aliases remain metadata observations. The current bound
Chrome channels and Chrome DevTools MCP 1.10.1 persistent default/CLI channel
roots follow the owner source. Custom roots and whole isolated/temporary profiles
remain advisory. Native use, actual byte accounting and adversarial fixture
results are recorded separately below. The earlier b08 run failed on worker.js
preservation (21 passed, one failed); the negative fixture was retained and
extended to case variants and runtime artifacts. On October 2 KST,
`cargo test -p neati-desktop --lib final_owner_boundary` passed all three focused
regressions: renderer state and forged selection, Corepack version-marker binding,
and Arc generic-scope/planner bypass protection. The complete browser suite and
final integrated source have not been rerun, so this is focused boundary evidence.

CloudKit/Safari/index and system roots are evaluated individually in the parent
[system access matrix](../../SYSTEM_CLEANUP_ACCESS.md);
that assessment distinguishes current-user metadata access from native TCC and
mutation authorization. Further SwiftPM/Conda/mise compatibility and the native
permission transition matrix remain in the existing four-batch integration.

## Additional tool compatibility research checkpoint

Actual task-owned mise 2026.9.18 official macOS arm64 `doctor --json` and
`cache clear` runs passed for both the default and an explicit external fixture
cache root. Each removed four disposable sentinels and preserved installed tools,
authored configuration, trust state and project outputs. These are additional
owner-command observations; the production adapter was not invoked from the
nonstandard fixture installation root.

The Swift.org 6.4.0 RELEASE signed package's extracted subset still reports the
existing exact banner `Swift Package Manager - Swift 6.4.0-dev`. Its fixed
`purge-cache` command removed repositories, registry/downloads and
manifests/manifest.db, preserving seven protected sentinels. This supplies a new
actual distribution observation for the existing banner and scope, rather than a
new accepted banner. The production adapter was not exercised from that fixture
installation root.

The official Conda 26.9.0 PEP distribution, installed into a task-owned Miniforge
runtime, reported its version successfully, but the exact nonmutating clean
preview exited 1 at the `main_pip` initialization gate. This particular
distribution remains advisory and fails closed. The result neither expands
cleanup compatibility nor proves that every Conda 26.9.0 distribution is
unsupported; the separately validated existing distributions remain unchanged.

Pinned public source, artifact digests, package-signature evidence and execution
receipts are preserved in the recovery archive. Final compatibility fixture and
adapter integration remains a PR TODO. No global tool or system toolchain was
installed, and no real user cleanup occurred in these experiments.

## Current namespace remeasurement

[The bounded metadata receipt](residual-namespace-metadata.json) recorded only
named roots and one bounded existing-repository bytecode observation on October
1. No contents, credentials, symlinks, arbitrary project search or mutation were
used. Shell access is not the native app's TCC verdict. Roots can overlap; their
amounts are not summed or presented as reclaimable disk space.

| Scope | Current allocated observation | Resulting decision |
| --- | --- | --- |
| Default Oh My Zsh cache | 16,384 B, complete within bounds | Generated/downloaded shell completion and update state remain advisory under the established owner evidence. |
| Current-user Google Updater CRX root | 4,096 B, complete within bounds | Existing index/archive owner format and use guards still decide eligibility; this is not the historical 754.4 MB dataset. |
| Default OpenCode XDG root | Existing empty directory, 0 B in this namespace | Executable/offline package and model metadata contract remains advisory. No claim about custom roots or all installations. |
| Existing clean1 bytecode | 36,864 B, one directory/four files, **partial** depth-bounded coverage | Remains advisory; no arbitrary project-wide bytecode sweep or claim that the historical row was exactly identified. |
| Exact default SDK staging/Corepack/Arc/MCP leaves probed | Absent in the named scopes | A scope observation only; no whole-profile/app absence, cleanup authority or GB-scale coverage benefit is inferred. |

## TODO before completing the issues

- [ ] Integrate the C scan diagnostics/cancellation hook and preserve one copy of
  its default trait method. Browser inventory cancels between profiles/units;
  a unit's metadata measurement/inspection is not immediately interruptible.
  Its fingerprint has a ten-second budget; do not claim bounded sub-millisecond
  cancellation for real large browser units from fixture Stop timings.
- [ ] Finish the broader browser-layout decision matrix (including Dia 1.50.1,
  Firefox and Antigravity) and the remaining per-class protected/unsupported
  decisions. No unverified profile or installed distribution becomes actionable.
- [ ] Run a task-owned actual Chromium profile and native exact-use busy/idle
  check when native validation resumes. Existing process/handle callback tests
  are fixture evidence; no real browser or user profile was cleaned.
- [ ] Promote the archived mise/SwiftPM/Conda provenance and exact positive or
  negative distribution records into the existing compatibility fixtures/docs.
  Validate the production adapter with its supported launcher/root contract
  before changing supported distributions; task-owned command execution alone
  does not establish adapter compatibility. Conda's initialization gate stays
  advisory, with no bypass or blanket version claim.
- [ ] Complete the unsigned app grant/deny/revoke/relaunch/replacement matrix
  with the native acceptance batch and record final app version/OS/date.
- [ ] Run required Rust/frontend/architecture/version checks, final app build,
  bundle version/icon verification and report CI separately after integration.

Xcode #350, CLI #335, new Windows/Linux product work and privileged helper/signing
remain owner-deferred. An unsupported/protected decision above does not claim a
shipped cleanup adapter. Final local verification, native evidence and CI are
incomplete at this checkpoint, so these issues are not marked complete.
