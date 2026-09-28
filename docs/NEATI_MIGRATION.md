# neati identity contract (0.3.74)

Issue #338 establishes one neati identity before official distribution. The
owner requested no old-name aliases, detection, migration or compatibility
branches. The approved B symbol, handwritten wordmark, native glass and cleanup
policy are unchanged.

| Surface | Identity |
| --- | --- |
| Bundle / Tauri persistence | `com.neati.desktop` |
| Cargo packages | `neati-core`, `neati-platform`, `neati-desktop` |
| Rust library / private desktop executable | `neati_lib` / `Neati` (not installed on PATH; `neati` is reserved for the CLI) |
| npm / internal brand asset | `neati` / `neati.svg` |
| macOS credential service / account | `app.neati.ai.{provider}` / `neati` |
| Windows credential target | `NeatiAI:{provider}` |
| macOS log | `~/Library/Logs/Neati/neati.log` |
| Windows log namespace | `Neati/Logs` |
| Repository | `jaeyoung0509/neati` |
| Release artifacts | `neati-macos-arm64.dmg`, `neati-windows-x64-setup.exe`, `neati-windows-x64-setup-machine.exe` |
| Generated SBOM / WinGet metadata | `SBOM-neati.spdx.json` / `jaeyoung0509.Neati` |

## Personal installation

Settings and provider authentication may need to be configured again. Existing
data and credentials in other namespaces are neither imported nor deleted.
Review Full Disk Access / Files and Folders for neati and restart the app after
granting permissions; automatic permission continuity is not promised.

`just release` builds and installs the app; `just install-release` installs an
already-built release bundle. The installer validates `com.neati.desktop` and
uses transactional replacement with rollback. It does not inspect other app
names. A mismatched bundle already named neati.app is refused: move that bundle
to a backup location before installing. The Windows installer uses its ordinary
neati installation flow without a rebranding hook. Fixture tests never touch
real installed applications or credentials.

No automatic updater, data migration or background agent is introduced.
Avoid running two cleaner builds concurrently. IPC provider IDs unrelated to
the application identity keep their existing semantics.

## External distribution

Source metadata is not an external registration. Verify SignPath configuration,
signing identity and Apple notarization requirements before public release.
No new certificate or notarization is claimed. Homebrew `neati` / `neati-cli`
and the generated WinGet identity are not claimed to be published. Domain/name
clearance and publication remain separate tasks; the agent CLI belongs to #335.
Do not bypass Gatekeeper or clear quarantine.

Historical documentation has product spellings normalized for this rename.
Its original versions, dates, commit IDs and screenshot pixels remain historical
evidence, not fresh validation of 0.3.74.
