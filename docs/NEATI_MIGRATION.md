# Zenith → Neati (0.3.73)

Neati is the new public name of Zenith, not a separate cleanup engine.
The user approved the original handwritten B mark on September 28, 2026.
Cleanup policies, native glass, saved navigation and provider IDs are unchanged.

## Identity contract

| Changed | Retained intentionally |
| --- | --- |
| Product/window/tray/Settings names: Neati | Bundle ID: `com.zenith.desktop` |
| `Neati.app`, desktop binary `Neati` / `Neati.exe` | Tauri configuration/data identity, settings schema |
| Original n icon and handwritten header wordmark | `app.zenith.ai.{provider}` and Windows `ZenithAI:{provider}` credentials |
| Windows install folder / Start Menu: Neati | Existing Zenith log directories and `zenith.log` |
| Installer and WinGet display name: Neati | Rust packages, library, IPC enum spellings, private npm package name |
| Canonical icon: `src-tauri/icons/neati-mark.svg` | Repository URL and versioned `Zenith-*` downloadable artifact names |

The retained artifact names and `jaeyoung0509.Zenith` WinGet identifier avoid
breaking existing references. Their display metadata identifies Neati.
The internal brand registry key/file `zenith` now resolves to the Neati artwork;
`neati` is an alias. Historical screenshots/evidence remain historical, not
rewritten as new QA.

## Manual upgrade, not an automatic migration

There is no automatic updater. This PR does not install, publish or sign a
release. Do not run both names at once: they share settings and credentials.

- macOS: quit Zenith, move the old application bundle out of Applications to a
  backup location, then install Neati.app. Keep the backup until verified. Do
  not delete configuration, logs, credentials or caches. The local installation
  recipe refuses a remaining Zenith.app instead of deleting it or leaving a
  duplicate. It verifies the new bundle ID and refuses a foreign Neati.app.
- Windows: uninstall the old Zenith application through Windows Settings before
  installing Neati. Keep application data and credentials. The Neati NSIS hook
  refuses an old Zenith uninstall registration owned by this publisher, in
  either user or machine scope; it does not run a registry UninstallString.
  New Neati versions use their own normal NSIS upgrade path. This transition is
  intentionally manual, including when an installer is run silently.
- Permission continuity is not guaranteed by a stable bundle identifier. If
  macOS refuses a protected location after the move/signature change, review
  Neati's Full Disk Access / Files and Folders grants and restart the app. CLI
  permission design is separate in #335.
- Launch at login remains Planned; no new login item or background agent is
  installed by the rename. Existing user-created shortcuts may need updating.

Fixture tests exercise local installer replacement, first installation,
rollback, foreign-bundle refusal, and legacy-conflict preservation. Windows
installer execution must be validated by Windows CI; macOS tests do not prove
Windows migration or TCC behavior. Never claim existing credential access was
tested by reading user secrets.

## Distribution gates

Homebrew cask `neati` and formula `neati-cli` are proposals, not published
packages. No domain ownership or trademark clearance is claimed. Follow #334's
remaining naming and signing/notarization gates before a public Neati release.
Do not disable Gatekeeper or clear quarantine as an installation workaround.
CLI implementation belongs to #335; the desktop `Neati --doctor` is not that
future `neati` agent CLI.
