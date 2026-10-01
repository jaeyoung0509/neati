# User-tool cache discovery: first #381 slice

This change adds metadata-only advisory discovery, not cleanup operations.
It is the first bounded part of [#381](https://github.com/jaeyoung0509/neati/issues/381),
which remains open for owner-operation verification and further named stores.
It starts from neati `develop@ddcfe3937e5380448dcde1d53036ff209352d39f`
(0.3.89). The broader Mole comparison and its measurement limitations are
recorded in that issue; source-only differences are not evidence of a GB-sized
reclaim gap on this machine.

## Independently verified vendor contracts

| Store | Contract | Shipped discovery | Authority |
| --- | --- | --- | --- |
| Oh My Zsh | [`oh-my-zsh.sh` at 4d4cfc2](https://github.com/ohmyzsh/ohmyzsh/blob/4d4cfc287e9d887b81242c0e431b5f49f9cec5c1/oh-my-zsh.sh) defaults its cache to `$ZSH/cache`, falls back to XDG `oh-my-zsh` when unwritable, and adds `completions` to the shell function path. | `~/.oh-my-zsh/cache` and `${XDG_CACHE_HOME}/oh-my-zsh` on macOS/Linux. Both may exist; their observation does not identify the active shell's cache. | Advisory only. |
| GitHub CLI | [`go-gh/config.CacheDir` at v2.16.1 (37aa5bb)](https://github.com/cli/go-gh/blob/37aa5bbaf1a591aa134913efeb3775f0235dc5ab/pkg/config/config.go) chooses XDG `gh`, then platform/default roots. [`gh config clear-cache` at fc4b137](https://github.com/cli/cli/blob/fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd/pkg/cmd/config/clear-cache/clear_cache.go) removes the entire effective directory. | `${XDG_CACHE_HOME}/gh`, defaulting to `~/.cache/gh`, on macOS/Linux. The public label names a local cache, never GitHub Actions caches. | Advisory only; no `gh` command is executed. |

Oh My Zsh's [GitHub CLI completion plugin](https://github.com/ohmyzsh/ohmyzsh/blob/4d4cfc287e9d887b81242c0e431b5f49f9cec5c1/plugins/gh/gh.plugin.zsh)
writes executable shell completion code under the cache. Its
[upgrade checker](https://github.com/ohmyzsh/ohmyzsh/blob/4d4cfc287e9d887b81242c0e431b5f49f9cec5c1/tools/check_for_upgrade.sh)
reads `.zsh-update` as shell state. Those facts do not authorize clearing an
arbitrary cache tree or bypassing executable/structured-state protections.
The implementation and fixture tests are original neati code; no Mole
implementation is imported or translated.

## Scope and accounting

- No recursive home or `.cache` census, shell/config execution, response-body
  parsing, credential-content reading, owner command or filesystem removal.
- XDG resolution uses the existing `PlatformEnvironment` composition snapshot.
  Custom absolute XDG bases select only the fixed child namespace; invalid
  relative/traversing overrides do not silently select the default.
- Shell-local `ZSH`/`ZSH_CACHE_DIR` overrides are not visible from a desktop
  composition snapshot and are deliberately not inferred by reading `.zshrc`.
  Standard installation and fallback roots are named observations, not a claim
  of complete effective-root discovery.
- An injected XDG base may alias configuration or another installation store.
  All observations remain non-actionable regardless of layout, owner state or
  current use. The catalog's Manual strategy independently refuses a forged
  cleanup selection.
- Fixed Manual roots now reuse anchored component validation before measurement.
  A linked ancestor cannot cause an observation to traverse outside its named
  scope. Missing paths stay absent, while failed inspections remain unknown.
- Existing no-follow/depth/cancellation/overlap rules remain in force. Unknown
  or partial bytes are not a confirmed zero, and observed bytes never contribute
  cleanup authority or reclaimed free space.

## Fixture verification

`src-tauri/tests/user_tool_observation_tests.rs` uses disposable homes to cover
exact roots, XDG default/custom/invalid roots, config/credential/extension/shell
sentinels, executable completions, update state, unknown payloads, forged
selection, absent/empty stores, an inaccessible ancestor, depth/cancellation
bounds and Unix root/ancestor/descendant links. It does not clean user files.
The Manual-root guard also applies to existing fixed advisory catalog entries;
the full existing scanner/safety suites remain required checks.

Initial local verification on October 1, 2026, macOS 27.0.1 (26A434), app 0.3.90:

- The 13 focused discovery/safety fixtures passed, including counting an HTTP
  response fixture whose body has no read permission. Existing catalog,
  scanner, safety, hygiene, core and platform tests passed.
- `cargo check --locked --offline --workspace`, workspace/all-target Clippy
  with warnings denied, format, architecture and synchronized-version checks
  passed. `pnpm check`, all 472 frontend tests, `pnpm build` and icon drift
  verification passed.
- Full workspace tests with `--no-fail-fast` completed with 1,281 passed,
  3 failed and 5 existing ignored tests. The three unchanged failures are the
  OpenRouter loopback callback fixture (`bind` returns `EPERM`), the doctor CLI
  fixture (`log_writable` fails at the sandbox's read-only user log directory)
  and the developer-port Python loopback fixture (`bind` returns `EPERM`).
  Tests were not weakened or gated to hide these failures.
- `just build-fast` produced the isolated debug `neati.app`. Its short/build
  versions both read 0.3.90, packaged `icon.icns` matches the source SHA-256,
  and the current primary frontend asset keys are embedded in `Neati`.
  The installed/running app was not replaced or launched.
- No live cache scan, owner cleanup, Linux runtime session, Windows runtime
  session or native visual comparison ran. No frontend layout or assets change
  in this slice. These local results predate publication; subsequent CI and
  follow-up verification are recorded below.

### Windows CI follow-up

[PR #387's first Windows Rust job](https://github.com/jaeyoung0509/neati/actions/runs/36800853141/job/110174615949)
failed two of the ten discovery fixtures. One compared a fixture's mixed Windows
separators with the scanner's normalized display string. It now compares paths
through the environment's shared path algebra. The other exposed Windows'
`NotFound` result for a child beneath an existing regular file, which had turned
a blocked Manual namespace into a fresh absent observation.

The Manual-root scanner now qualifies that first missing component by inspecting
only its direct parent without following links. A file, link or inspection
failure remains an explicit gap; a directory or genuinely absent parent preserves
the existing absence behavior. No new cleanup authority or Windows cache root is
introduced. A fixture injects the Windows-style missing result on every runner,
and another verifies genuine absence independently of the host's error mapping.

Follow-up local tests on the same macOS build and app version passed all 13
discovery fixtures, all 25 scanner-walker tests, and the complete workspace
all-target suite: 1,287 passed, zero failed and five existing ignored. The earlier
loopback and log-write restrictions were absent in this verification environment.
Workspace doc-tests, check, Clippy with warnings denied, format, architecture,
version synchronization, frontend typecheck and all 472 frontend tests also
passed. `just build-fast` rebuilt the debug app with the current production
frontend; its short/build versions are both 0.3.90, its icon matches the source,
and the index's referenced assets are embedded. The installed app was not
replaced or launched. An actual Windows rerun and CI packaging results remain
separate checks.

The subsequent exact-head [Windows Rust job for d965806](https://github.com/jaeyoung0509/neati/actions/runs/36802481989/job/110179670771)
succeeded. Its ten user-tool observation fixtures, including the two previous
failures, and both portable scanner-walker regressions passed. The macOS package
smoke job for the same run also succeeded. Windows packaging completion was not
reviewed after the owner changed the merge workflow to use final local checks.

### Final target synchronization

The final tree incorporates `develop@9a5e1db189ff732abfc8af4d6f9334ad136d1c44`
(0.3.91), retaining its prepared aged-tree policy, fresh payload/age checks and
scan timings alongside this Manual-root boundary. The only merge conflicts were
the four version files: the complete target files were adopted, then
`just bump-patch` synchronized all manifests and workspace lock entries to
0.3.92. No additional production scope was introduced.

Final local verification on the same macOS build passed the full workspace
all-target suite: 1,293 passed, zero failed and six existing ignored tests,
including both the prepared-policy freshness and Manual missing-parent
regressions. Workspace doc-tests, check, format, all-target Clippy with warnings
denied, architecture, version and icon checks passed. The frontend typecheck
reported zero errors/warnings and all 479 frontend tests passed. Final-head CI
remains separate from these local results and is not awaited under the owner's
approved merge workflow.

`just build-fast` also passed, including the production frontend build. The
final debug bundle's short/build versions both read 0.3.92; its packaged icon
SHA-256 matches the source, and the current index asset names are embedded in
`Neati`. The installed/running application was not replaced or launched.

## Remaining #381 work

- Resolve supported effective shell cache overrides through a verified platform
  contract without executing user configuration or granting broad roots authority.
- Validate specific installed `gh` versions and the complete owner command scope,
  executable/root/inventory identity, fresh process/handle checks and final
  pre-mutation verification in disposable homes. No generic deletion fallback.
- Review Windows GitHub CLI path semantics separately: upstream's
  `LocalAppData/GitHub CLI` is also used for data/state. No Windows cache root
  or operation is advertised by this slice.
- Classify Kubernetes metadata, pyenv downloads, PyInstaller, pre-commit and
  user cloud logs from their own vendor contracts before further discovery or
  lifecycle operations. AWS credential caches and Prometheus WAL stay protected.
- Remeasure added observed bytes separately from cleanup yield. This PR runs
  fixture checks, not a live scan, live owner command or throughput benchmark.

Xcode #350 remains open/deferred and CLI #335 remains closed/not planned.
No release, tag or app installation is part of this change.
