# Framework output review (#391)

Discovery does not grant authority. Direct regular manifests identify SvelteKit
and Next; ordinary Vite/Svelte packages and names alone do not. Project JavaScript
is never imported, evaluated or executed. Inert config authority accepts only a
complete unique-key JSON object in the recorded loader's fixed export syntax and
package module mode. Configs are capped at 64 KiB, depth 16 and 1,024 members;
relative custom hints are capped at 16 paths, eight components and 256 characters.
Dynamic, custom, ranged-version, ambiguous and unsupported layouts retain observed
bytes and evidence with cleanup unavailable. Literal JS strings are hints only.
Next's actual find-up loader also binds a 32-level ancestor absence search when
no supported direct config exists; an inherited config cannot grant whole authority.

Whole removal supports **SvelteKit 2.37.1's recorded sync-only variant** and
**Next 15.5.14's uncompressed Webpack v1 cache-only default**. Svelte's complete
recorded route/env declarations and default tsconfig are checked; other declaration
variants stay observed. Next permits only `cache/webpack` and its positive pack
format (plus the exact generated package marker). Every entry is inspected;
deployment/server/static/BUILD_ID, offline/fetch/image state, authored suffixes,
unknown files, nested repositories, links, executables and tracked content prevent
whole authority. Existing verified generated children remain separate choices.
The issue explicitly permits unknown/custom layouts to remain advisory; this is
not authority for arbitrary whole build or deployment output.

| Original criterion | Implementation and fixture proof |
| --- | --- |
| Typed direct recognition | Central `developer_artifacts/rules.rs`; `framework_defaults_require_exact_direct_bounded_dependency_metadata` and linked-manifest tests. |
| Bounded metadata; custom/unknown treatment | `framework_metadata.rs`, strict core parser; config mode/duplicates/links/size/version tests and `custom_dynamic_outputs_stay_observed_and_nested_projects_stay_discoverable`. |
| Observed accounting, manual review | Logical/allocated bytes stay visible; no default selection; `actual_generated_default_whole_units_move_with_exact_accounting_and_overlap_refusal`, frontend exact preview/overlap tests. |
| Whole mutation safety | Private typed snapshot, fresh manifest/config identity/content, positive whole-tree contract and #390 provenance at planning and execution; descriptor staging, final project-wide use, root/ancestor/stage/leaf rebind, identity-only rollback. |
| Nested/deduplicated/bounded discovery | Existing wrapper/vendor/workspace regressions, linked Git worktree tests, `unverified_default_framework_outputs_preserve_nested_package_descent`, candidate/cancellation/byte budget tests. |
| Broad negative/forged matrix | Default fixtures; deployment/offline/unknown/nested Git/authored sentinels; tracked/config/manifest/link/compressed replacements; forged advisory planning **and execution**; actual project CWD and injected unknown/final-marker native boundary tests. |

The existing 16-workspace, 512-candidate and 250,000-entry limits remain bounded.
Native fixtures move only task-owned temporary trees to a fixture Trash directory;
source and manifest sentinels remain. Staged Git checks retain the original index
namespace, so renaming tracked contents cannot make them appear untracked. macOS
use checks reject active and unknown evidence; only exact self PID/inspection FDs
are excluded. Descendant ctime detects same-length edits with restored mtime.
Selecting a whole folder and its child is refused; stale related rows are removed
after a move. Review shows original restore paths because Put Back may refer to
private staging. Failure details preserve the accepted object and show recovery.

Actual generated files and SHA-256 provenance are tracked under
`src-tauri/tests/fixtures/frameworks/{sveltekit-2.37.1,next-15.5.14}`. They were
generated in an isolated owned project with SvelteKit sync and Next's bundled
Webpack 5.98.0, not copied from Mole or a user project. Sources:
[SvelteKit structure](https://svelte.dev/docs/kit/project-structure),
[recorded loader](https://github.com/sveltejs/kit/blob/%40sveltejs/kit%402.37.1/packages/kit/src/core/config/index.js),
[Next distDir](https://nextjs.org/docs/app/api-reference/config/next-config-js/distDir),
[recorded config names](https://github.com/vercel/next.js/blob/v15.5.14/packages/next/src/shared/lib/constants.ts).

Native whole-output use/removal is macOS-only. Windows/Linux lack this owner-use
adapter and remain unavailable for framework mutation; pure metadata fixtures do
not establish native parity. Focused core/platform, developer/Trash safety suites,
final loader boundary cases, workspace Clippy, architecture, format, typecheck and
20 API/UI fixture tests passed locally on macOS. This source slice retains 0.3.100; root performs
reserved version integration, final whole-suite/bundle/icon checks and final browser
evidence. No real user cleanup, installation or native desktop UI QA was performed.
