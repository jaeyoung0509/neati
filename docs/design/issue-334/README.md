# Neati identity review — issue #334

Status: user approved B on September 28, 2026; implementation uses its original
path and rounded stroke. Starting revision: `fc3c0ce`, version
0.3.72, September 28, 2026.

## Approved direction

Neati (니티), with a readable handwritten lowercase wordmark and an original,
rounded lowercase n symbol. Warmth comes from proportions and curves, not a
face or cleaning props. Retain native glass, existing layouts and semantic
colors. Handwriting is for the wordmark, not interface text.

`concepts.svg` preserves two original vector sketches and one shared hand-drawn
wordmark. A favors a clear arch and compact silhouette; B adds a flowing pen
gesture. Both include compact and monochrome previews. The headline on the
sheet describes this study; it is not an approved product tagline. Color and
lettering remain subject to approval and optical refinement.

The generator now validates the approved single stroked n path instead of three
filled Z paths. Its monochrome template retains the exact stroke, caps and
geometry. Native packaging rasterizes the vector at each target size; no
third-party font is needed. See `docs/NEATI_MIGRATION.md` for the final map.

## Identity map and implementation gates

| Surface | Current evidence | Intended treatment |
| --- | --- | --- |
| Product and windows | `src-tauri/tauri.conf.json`: Zenith / Zenith Quick | Neati / Neati Quick |
| App bundle | Product name produces Zenith.app | Neati.app; no replacement of installed app without approval |
| Desktop executable | `src-tauri/Cargo.toml`: Zenith | Neati; audit scripts, self-process protection, packaging tests and doctor commands together |
| Bundle identifier | `com.zenith.desktop` | Retain for continuity; do not assume this guarantees TCC continuity |
| Rust packages/library | zenith-core/platform/desktop, zenith_lib | Retain; not public branding |
| Config location | Tauri `app_config_dir()` in desktop composition/commands | Preserve identifier and verify resolved paths on each platform |
| Keychain service | `app.zenith.ai.{provider}` | Retain; no secret export or migration needed for a display rename |
| Windows credential target | `ZenithAI:{provider}` | Retain |
| Logs | `src-tauri/src/diagnostics/mod.rs`, `zenith.log` and legacy directory rules | Retain existing paths; distinguish display text from persistence |
| Windows Start Menu | Previously Zenith | Neati; legacy-install guard requires manual old-app removal |
| Release artifact names | Explicit Zenith names in release/CI workflows | Preserve existing public artifact contract unless a reviewed transition is added; do not silently break URLs/checksums/WinGet |
| Repository URLs | jaeyoung0509/zenith | Retain; no repository rename |
| Homebrew | Proposed cask neati / formula neati-cli | Document only; no tap creation/publication in this work |
| CLI | Follow-up #335 | Reserve neati in design only; no CLI implementation here |

This is the initial source-inspection map, not completed upgrade QA. The final
contract and remaining release gates are in `docs/NEATI_MIGRATION.md`.

## Review gate and verification

The user approved B; A is retained only as historical design context. Version
0.3.72 → 0.3.73 uses the repository patch recipe once for this PR.

No app build, installation/upgrade test or native glass QA is claimed by this
concept sheet. Final evidence must separately cover packaged icon/version,
native main/quick/tray/Finder surfaces, small-size silhouettes, and platform
limitations. Naming/domain/trademark release gates in #334 remain open.
