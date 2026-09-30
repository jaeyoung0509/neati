# Owner command compatibility — issue #362

2026-09-30 · macOS 27.0.1 (26A434) · arm64 · neati 0.3.86 → 0.3.87.

This batch validates installed owner tools in disposable homes. It corrects
CocoaPods' command scope and standard user RubyGems loading. Xcode #350 remains
pending the owner's setup; no Xcode installation or permission change occurred.
The other owner adapters and native permission-transition work in #362 remain open.

## CocoaPods findings and correction

The installed 1.16.2 distribution demonstrated two gaps in the earlier facade:

- Isolated HOME hid `~/.gem/ruby/2.6.0`, so the pinned gem could not load.
- [Command::Cache](https://github.com/CocoaPods/CocoaPods/blob/1.16.2/lib/cocoapods/command/cache.rb)
  constructs its downloader at `Config.cache_root + 'Pods'`. The earlier preview
  measured the parent `CocoaPods` directory. A fixture with parent Specs/VERSION
  produced a partial outcome after the real command removed only its Pods child.

The preview, runtime command and post-check now agree on
`~/Library/Caches/CocoaPods/Pods`, including Release, External, Specs and VERSION.
Parent siblings remain outside the command and its estimate. Preview never
constructs the downloader; unknown versions/layouts, links and locks still block.
Repositories, project Pods, Podfile/lock, configuration, credentials and tools
remain preserved. The operation uses the existing explicit permanent-deletion
confirmation, with fresh recursive identity, owner and handle checks.

The standard user gem repository is derived only from the reviewed absolute
`~/.gem/ruby/<ABI>/bin/pod` launcher, checked for links and matched against its
Ruby ABI. HOME/configuration/repositories stay isolated. Caller GEM_HOME,
GEM_PATH, RUBYOPT and Bundler settings are not inherited. Loaded gemspecs join
the interpreter and libraries in the runtime fingerprint.

The Ruby subprocess regression now includes the unmodified upstream cache
constructor as well as its removal method, and checks parent-sibling sentinels.
Additional regressions refuse custom gem layouts, env shebangs, links and ABI
mismatches; existing runtime replacement, changed review, busy/unknown handles,
unknown cache versions and observation-only failure cases remain.

## Installed tools exercised

Production adapters ran scan → prepare → execute → post-check against isolated
installations under `/private/tmp/neati-*-validation-*`. Process observations
were an idle fixture for those installations. No actual user cache was cleaned.
The functional runs used 0.3.86 with this PR's source changes, before the one
patch bump; the final complete verification and bundle use 0.3.87.

| Tool | Recorded version | Candidates / roots | Allocated bytes removed | Remaining candidate bytes |
| --- | --- | ---: | ---: | ---: |
| Miniforge / Conda | 26.3.2-3 / 26.3.2 | 81 | 39,555,072 | 0 |
| Miniforge updated / Conda | 26.7.2-0 / 26.7.3 | 114 | 130,072,576 | 0 |
| mise | 2026.9.14 | 3 | 28,672 | 0 |
| mise | 2026.9.17 | 3 | 40,960 | 0 |
| CocoaPods / system Ruby | 1.16.2 / 2.6.10p210 | 1 | 20,480 | 0 |
| SwiftPM, installed Command Line Tools | Swift 6.4.0-dev | 3 | 24,576 | 0 |

Conda/Python and mise executables, mise installations/configuration/trust,
CocoaPods launcher/repositories/project/settings/credential sentinels and
SwiftPM artifacts/prebuilts/configuration/security sentinels remained present.
Measured fixture bytes are owner removals, not a measured disk free-space delta.

Together with the [0.3.80](../cleanup-0.3.80/README.md) and
[0.3.81](../cleanup-0.3.81/README.md) runs, the recorded sets are Conda
26.3.2/26.5.3/26.7.2/26.7.3 and mise 2026.9.14/15/16/17. This does not assert
every intervening or future release. Complete response/scope checks still apply;
there is no generic filesystem fallback. SwiftPM's supported banner is unchanged.

CocoaPods was installed from RubyGems, with RubyGems 3.0.3.1 and Bundler 2.4.22.
[Gemfile](Gemfile) and [lockfile](Gemfile.lock) record the dependency set.
ActiveSupport 6.1.7.10, concurrent-ruby 1.3.4 and ffi 1.17.0 were selected for
system Ruby 2.6. An unpinned concurrent-ruby installation failed upstream logger
initialization; no production workaround or general dependency claim is added.
The CocoaPods cached gem was installed locally with `--no-env-shebang` to generate
the standard absolute RubyGems launcher. The cache target stayed separate from
the distribution. [Evidence](evidence.json) records release-verified download
SHA-256 values, the conda-forge package and the RubyGems-verified CocoaPods hash.

Validation executables:

```sh
cargo run -p neati-desktop --example validate_tool_cleanup -- <neati-cli-validation-root>
cargo run -p neati-desktop --example validate_cocoapods_cleanup -- <neati-cocoapods-validation-root> 2.6.0
cargo run -p neati-desktop --example validate_swiftpm_cleanup
```

The first two expect tools installed in their disposable home, and deliberately
fail on missing/empty fixtures. CocoaPods accepts only a canonical direct
`/private/tmp/neati-cocoapods-validation-*` root. Fresh CocoaPods installations
should use the recorded lockfile rather than updating an existing gem repository.
Homebrew/env launchers, custom gem homes, other Ruby/CocoaPods versions and other
Swift toolchains remain unverified.

## Final verification

- `cargo check --workspace`: passed.
- `cargo test --workspace`: 1,262 passed, five existing ignored, zero filtered.
- `just lint-rust`: format and Clippy with warnings denied passed.
- `just check-architecture` and `just check-version`: passed.
- `pnpm check`: zero errors/warnings; `pnpm test -- --run`: 465 passed in 48 files.
- `pnpm build`, `pnpm icons:check` (61 assets/registry entries) and final
  `just build-fast`: passed.
- Bundle versions: 0.3.87 / 0.3.87; packaged executable matches the debug binary
  and packaged ICNS matches the source. Bundled `Neati --doctor`: 14/14 passed.

CI is reported independently on the PR; Windows runtime/packaging were not run
locally. No UI/assets changed, so no screenshots are required. The installed
application remains 0.3.85; the running/installed app was not replaced. No release,
tag, signing change or real user cleanup was performed.
