# CocoaPods owner cleanup validation

September 30, 2026 · neati 0.3.82 → 0.3.83 · macOS 27.0.1 (26A434), arm64.

This follow-up addresses the CocoaPods item in section B of #362. Other issue
items remain open. It adds a reviewed permanent owner operation through the
existing confirmation UI; no frontend component, layout or asset changes are
included, so there are no new UI screenshots.

## Tested owner contract

Six focused regressions passed. They cover whole-root scope, custom cache/home
roots, unknown tool/cache versions, unknown root entries, links, downloader
locks, changed Ruby files and reviewed inventory, unknown handles, private
confirmation/permanent-delete authorization, and blocked observation visibility.

The actual native subprocess command runs system Ruby 2.6.10 in a disposable
RubyGems fixture containing CocoaPods 1.16.2's unmodified MIT-licensed
[`cache/clean.rb`](https://github.com/CocoaPods/CocoaPods/blob/1.16.2/lib/cocoapods/command/cache/clean.rb).
A small facade supplies CLAide, UI and cache construction. The production
launcher disables plugin loading, pins the gem version, and supplies only the
fixed `cache clean --all --no-ansi --silent` arguments. Preview inventories Ruby
files without constructing a downloader cache. Mutation executes the upstream
whole-root removal method. Repository, project Pods, Podfile.lock, configuration
and credential sentinels remain unchanged. Test-only GEM_HOME/GEM_PATH point to
the disposable fixture and are not inherited by production commands.

This is a source-contract and subprocess fixture, **not validation of an
installed CocoaPods distribution**. CocoaPods is absent on the test machine;
RubyGems installation could not access rubygems.org under the restricted
network. Standard absolute RubyGems launchers are the initial supported layout;
Homebrew wrappers, env-based shebangs, custom gem homes and other versions still
need compatibility validation. A runtime that cannot load the pinned gem is
blocked and its default-store bytes remain observation-only.

## Local checks

- `cargo check --workspace --offline`: passed.
- `cargo test --workspace --offline`: attempted; existing native tests encounter
  sandbox restrictions. Loopback socket creation is refused in
  `a_silent_connection_times_out_and_a_later_callback_is_still_accepted` and
  `test_controlled_integration_ephemeral_loopback_release`; native diagnostics
  cannot write the real user log directory in `run_cli_exit_codes_follow_the_checks`.
- Full remaining workspace run with exactly those three test names filtered:
  **1,240 passed, five existing ignored tests, three filtered**, including all
  six new CocoaPods regressions. No test code was relaxed or skipped in CI.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo fmt --all --check` and `just check-architecture`: passed.
- `pnpm check`: zero errors and warnings.
- `pnpm test -- --run`: 456 tests passed across 48 files.
- `pnpm build` and `just build-fast`: passed; the latter rebuilt the final
  frontend and bundled the debug application.
- `just check-version`: all six manifest/lockfile version entries synchronized.
- `pnpm icons:check`: 61 icon assets and registry entries verified.
- Bundle inspection: version 0.3.83, packaged Neati executable present, packaged
  ICNS bytes match the tracked application icon.

No real user caches were cleaned. The running or installed app was not replaced.
CI and installed-distribution results must be reviewed separately; these local
checks do not claim Windows runtime behavior or a fully successful unfiltered
Rust suite in this sandbox.
