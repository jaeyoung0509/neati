# Cleanup scope and Quick Panel validation — 0.3.89

Issues [#362](https://github.com/jaeyoung0509/neati/issues/362) and
[#376](https://github.com/jaeyoung0509/neati/issues/376), one review unit.
Version **0.3.88 → 0.3.89**. Recorded September 30, 2026 on macOS 27.0.1
(26A434), Apple Silicon arm64. Xcode #350 and CLI #335 remain owner-deferred.

## Mole scope reconciliation

The owner ran Mole 1.55.0 themselves. Its operation log and the earlier neati
scan were inspected read-only. No cleanup, sudo command or permission change
was performed by this work. Paths below are namespace templates; private logs
and the raw scan ledger are not committed.

| Reported scope | Evidence and resulting behavior |
| --- | --- |
| GoogleUpdater CRX cache, 754.4 MB | A real missing scope: two entries in the exact current-user `GoogleUpdater/crx_cache`, separate from existing updater log cleanup. The new typed owner moves the whole positively verified download/index unit to Trash. Installed versions, updater preferences and other directories remain outside scope. |
| OpenCode, 263.5 MB | Actual `.cache/opencode` contained about 258.2 MB of packages and 5.3 MB of model-catalog JSON; the old catalog named only legacy `.opencode` roots. The configurable XDG root is now observed. Executable plugin/SDK and offline-use dependencies remain advisory; no installed-version purge contract was verified. |
| Chrome Service Worker, 1.23 GB | All 52 removed paths matched 52 earlier neati reviewable units, totaling 1,226,813,440 observed bytes. Discovery already existed. Owner review and fresh exact-use checks differ from Quick Clean eligibility; no generic browser-state deletion is added. |
| Chrome code-signature clone, 2.27 GB in the operation log | The terminal's “System” summary omitted this amount. Adds observation of the exact Chrome and IntelliJ namespaces below the current user's OS-resolved `X`. Executable update/signature dependencies remain protected. APFS cloning means this footprint is not proven unique physical recovery. |
| OrbStack, Rust toolchains and project artifacts | These were review/advisory lines in `mo clean`, not removed items. Their dedicated workflows and scope limitations remain. |

Mole's “Tracked cleanup: 1.26 GB” is not a complete byte total: the inspected
Service Worker and code-clone removal branches do not add to that counter.
The reported **+5.00 GB** is a global filesystem free-space change, affected by
other activity and shared storage. It is not a controlled neati/Mole comparison.
Since the owner already removed the original files, no fresh before/after
capacity or timing benchmark against that dataset is claimed here.

## Google Updater contract

The adapter accepts only the registered current-user `crx_cache` unit on macOS.
It requires the known `hashes → SHA-256 → appid` index, an exact matching flat
set of non-executable regular CRX3-header payloads, complete SHA-256 content
checks, current UID ownership, and no symlinks/hard links or group/world write
bits. Limits: 1 MiB index, 128 archives, 2 GiB payload, 20-second payload
verification budget. This is format/content validation, not CRX signature
authentication. The positive format fixture is synthetic; the previously
removed real 754.4 MB unit was not executed by neati.

Scan, preparation and execution obtain fresh owner/handle evidence. Execution
revalidates payloads, checks owner/handles again, then verifies every captured
entry identity before the whole-unit move. It uses the existing private
one-shot authorization and recoverable Trash adapter. Unknown formats and use
states retain measured bytes without granting cleanup authority.

Six macOS fixture tests cover complete movement and replay, preservation of
parent installed state, malformed/missing index or payload, extra state,
links, executable/shared permissions, active/unknown owners, byte budgets,
unknown UID, out-of-scope selection, empty units, and a payload rewritten by
the final handle probe. Temporary test data is the only mutation target.
Scanner fixtures also assert zero cleanable bytes and no selection for OpenCode
packages and code clones. Pure platform tests reject unsupported or malformed
`C`-to-`X` roots; accounting tests qualify clone bytes without authorizing deletion.

Primary contracts inspected:

- [Chromium CRX cache writer](https://github.com/chromium/chromium/blob/349ddf6b6b53e786568fe8c15bfa5d1c1483697f/components/update_client/crx_cache.cc)
  defines the coupled index, app IDs and hash-named payloads.
- [Chromium clone manager](https://github.com/chromium/chromium/blob/5856e4aef1055851e84dd673eedb6222f0ea4a91/chrome/browser/mac/code_sign_clone_manager.mm)
  and [contract](https://github.com/chromium/chromium/blob/5856e4aef1055851e84dd673eedb6222f0ea4a91/chrome/browser/mac/code_sign_clone_manager.h)
  describe staged-update code validation, COW cloning/hard links and owner-exit cleanup.
- [OpenCode package cache](https://github.com/anomalyco/opencode/blob/2fa3363c924c5c3e367b84a87ae478296a0ed59b/packages/core/src/npm.ts),
  [XDG paths](https://github.com/anomalyco/opencode/blob/2fa3363c924c5c3e367b84a87ae478296a0ed59b/packages/core/src/global.ts)
  and [model catalog](https://github.com/anomalyco/opencode/blob/2fa3363c924c5c3e367b84a87ae478296a0ed59b/packages/core/src/models-dev.ts).
  Upstream source is lifecycle evidence, not proof of a local installed release.

## Quick Panel behavior and mounted evidence

The window fits after settings and capability readiness, then retains its bounds
for the same saved layout, width and display work area, including repeat
openings. Transient readings do not trigger content fitting. Deliberate layout
or display changes refit through a debounced, serialized controller; obsolete
activation results are discarded. The shell uses its border box so its border
cannot cause repeated width reduction. Configured providers retain honest
placeholder rows, a reserved status line and a gauge slot. Unknown/stale usage
does not render an invented zero-percent meter. Longer rows overflow through
the existing body scroller; header/footer and native glass styles are preserved.

The real Svelte QuickPanel was mounted in Chromium with production CSS and a
test-only window port. Fixture aliases never enter the production build. The
port records actual component `setSize` requests and exercises its real
ResizeObserver, activation and subscription paths.

- [Mounted transitions](mounted-transitions.json): loading, partial, fresh,
  stale, protocol error, timeout, disconnected, empty and long-name states;
  a fixed **360 × 621 px** window/footer and two gauge slots, one initial resize.
  Ordinary content stays 499 px; the long-copy fixture grows to 604 px inside
  the scroller. Fresh usage has meters; loading/stale/error/empty usage has none.
- Repeat hide/reopen keeps fitted bounds and restores one subscription; hidden
  panels have zero usage subscriptions. Resolving a deferred old monitor probe
  after a new activation adds no resize.
- 320/360/400 px widths, explicit preferences and a 360 px work-area cap remain
  bounded; the constrained case scrolls with its footer reachable.
- [Reduced motion](reduced-motion.json) retains bounds through loading/fresh
  transitions. [Escape](keyboard-close.json) hides and unsubscribes the panel.
- Five fake-timer tests separately exercise fitting, deliberate refits,
  cancellation/disposal and serialization of overlapping native calls.

| Width | Light | Dark |
| --- | --- | --- |
| 320 px | [Screenshot](quick-320-light.png) | [Screenshot](quick-320-dark.png) |
| 360 px | [Screenshot](quick-360-light.png) | [Screenshot](quick-360-dark.png) |
| 400 px | [Screenshot](quick-400-light.png) | [Screenshot](quick-400-dark.png) |

[Constrained work area](quick-constrained.png).

Reproduce with the installed agent-browser CLI:

```sh
node scripts/build_quick_panel_validation.mjs
python3 -m http.server 5187 --bind 127.0.0.1 --directory /private/tmp/neati-quick-panel-validation
# In another terminal:
node scripts/validate_quick_panel.mjs
```

## Native and platform limits

A separate intermediate `com.neati.quick-validation` bundle was built and launched on the
recorded macOS build. Its embedded frontend displayed v0.3.89 and completed a
read-only scan, before the final status-line and failed-refresh gauge refinements.
It used separate safe settings and was then quit; the installed
`/Applications/neati.app` process remained running. No user app installation,
cleanup or privacy grant changed.

The automation surface did not expose the status-bar control. **Native Quick
Panel bounds/position/video and tray reopen were not verified.** Browser evidence
does not substitute for that check. No Windows runtime or monitor/DPI transition
was executed locally. Keep #376 open for native acceptance verification, and
#362 open for TCC transitions, additional owner operations and compatibility.
The final standard debug bundle is built separately with the production identifier.

## Local checks

Final check results are recorded below before PR handoff. CI is reported
separately in the PR and is not implied by local success.

| Check | Result |
| --- | --- |
| `cargo check --workspace` | Passed |
| `cargo test --workspace` | 1,271 passed; 0 failed; 5 ignored |
| `cargo fmt --all -- --check` and Clippy with `-D warnings` | Passed |
| `just check-architecture` | Passed; core/platform dependency boundaries intact |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test -- --run` | 472 passed across 49 files |
| `pnpm build` | Passed, including Tailwind production verification |
| Three ignored binding/capability/context export tests and generated-file drift | Passed; generated contracts unchanged |
| Python cleanup-analysis privacy tests | 12 passed |
| Mounted browser runner and reduced-motion/keyboard checks | Passed |
| `just check-version` and `pnpm icons:check` | Passed |
| Final `just build-fast` | Passed; production `com.neati.desktop` bundle v0.3.89 |
| Packaged icon | Byte-identical to `src-tauri/icons/icon.icns`; SHA-256 `6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9` |
| Packaged `Neati --doctor` | All 14 self-checks passed |

The production bundle was rebuilt after the final version/frontend changes.
It was not installed or used to replace the running app. Native signing,
permission persistence, Windows runtime and Quick Panel bounds remain unverified.
