# Browser layout decisions — October 2, 2026

This is a compatibility decision record for the 0.3.102 source checkpoint on
macOS 27.0.1 (26A434), arm64. Public bundle metadata and pinned source establish
only the facts stated below. They do not establish native profile activity,
Full Disk Access, deletion authority or Windows parity.

| Owner | Recorded evidence | Decision in this batch |
| --- | --- | --- |
| Arc | Official 1.166.0/build 87668, ArcCore Chromium 154.0.8037.58; public initialization binds `Arc/User Data`. | Keep the exact existing typed root/runtime contract. Legacy aliases and unknown versions remain observations. Generic Arc mirror cleanup cannot bypass that verdict. |
| Dia | Official 1.50.1/build 87750, bundle `company.thebrowser.dia`, executable/name `Dia`; embedded ArcCore Chromium 154.0.8037.58. Static initialization reads the bundle name to construct Application Support. | No typed Dia profile or whole offline-cache unit is enabled. A shared Chromium version and similar code do not prove Arc's runtime/root contract for Dia. No Dia process or actual profile-use test was run. |
| Firefox | Pinned Mozilla source below establishes HTTP-cache root selection and `cache2` layout. No installed Firefox distribution/profile was exercised in this batch. | No new macOS owner adapter or whole-profile operation. Custom/cache-parent overrides require their own bound discovery contract. Existing registered ordinary payload policies retain their scope and guards. |
| Antigravity | Installed public Info.plist: 2.15.1, bundle `com.google.antigravity`, executable `Antigravity`. Official documentation identifies authored artifacts, knowledge, settings and resumable conversation records under distinct application/CLI surfaces. | No new browser-profile or full CacheStorage operation. Retain authored outputs, knowledge, resume indexes, configuration, credentials, extensions and models outside new cleanup authority. Existing registered renderer-cache leaves keep their own narrow pipeline. |

The Dia metadata came from the [official distribution](https://releases.diabrowser.com/release/Dia-latest.dmg),
whose recorded DMG SHA-256 is
`1633666355bd1b79c4e5ff36607c8a98fb3a4103b2f300dc2c2694c47a234077`.
The recovery archive retains the manifest and static initialization receipts.
No Dia binary was downloaded or launched for the October 2 reconciliation.

Mozilla's [HTTP-cache description](https://firefox-source-docs.mozilla.org/networking/cache2/doc.html)
separates SHA-1-named entry files from the persistent index files. At revision
`084057e952e7dbf376f6c3765ad242aec3785dc6`,
[`CacheFileIOManager::OnProfile`](https://github.com/mozilla-firefox/firefox/blob/084057e952e7dbf376f6c3765ad242aec3785dc6/netwerk/cache2/CacheFileIOManager.cpp)
chooses a user override, the application cache-parent directory, or the local
profile directory, then appends `cache2`.
[`CacheObserver`](https://github.com/mozilla-firefox/firefox/blob/084057e952e7dbf376f6c3765ad242aec3785dc6/netwerk/cache2/CacheObserver.cpp)
and [`nsXREDirProvider`](https://github.com/mozilla-firefox/firefox/blob/084057e952e7dbf376f6c3765ad242aec3785dc6/toolkit/xre/nsXREDirProvider.cpp)
show that overrides and profile-local directories matter. These facts do not
classify cookies, logins, history, extensions or offline website state as HTTP
cache. The recorded source SHA-256 values are respectively
`36d3aba9c98462eaaa61f9c2fb131879a5d12620497bb1362cced87be06313a7`,
`181ab3da417f9236f1c5585d2bf95c3ccfe14b32fe6a6847fa8eb3b4847eb733`, and
`796c7570f506014bb8870e03636bb1124427e61c765f2d3104458c7ff1810b88`.

Antigravity's [agent settings](https://www.antigravity.google/docs/agent-settings)
and [settings](https://antigravity.google/docs/settings) describe different local
application-data paths; their equivalence across releases is unverified.
[Artifacts](https://www.antigravity.google/docs/artifacts/) include plans, diffs,
images and recordings. The [CLI resume command](https://www.antigravity.google/docs/cli/commands/resume)
uses `~/.gemini/antigravity-cli/cache/last_conversations.json` as a resume index.
A directory named cache is insufficient to authorize removing that state.
Only public bundle metadata was read; no user agent data was inspected.

## Native busy/idle acceptance remains unverified

The independently created Chrome 154.0.8037.59 test home failed before opening a
debugging endpoint: installed Chrome rejected the default profile under the
isolated home. No CacheStorage fixture, native lsof verdict, cleanup plan or
deletion was reached. `validate_chromium_unit_use` built successfully, but the
native busy-to-idle test did not run. Fixture callback regressions remain the
available use-policy evidence. The experiment did not use an existing browser
session or real profile and was not retried.

Chrome startup attempted GoogleUpdater registration inside that fresh test home.
The positively identified task-owned launch-agent and updater directory were
removed after their ancestry/program path was checked. The browser and updater
processes exited; no task process remains. Real user directories were unchanged.
The failure and cleanup receipts remain in the recovery archive. Native neati
grant/deny/revoke/relaunch/replacement validation belongs to the separate native
acceptance batch and is not supplied by this experiment.
