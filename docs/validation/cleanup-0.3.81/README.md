# Cleanup 0.3.81 validation

Date: September 29, 2026. Host: Apple Silicon, macOS 27.0 (26A428).
Related issue: [#362](https://github.com/jaeyoung0509/neati/issues/362).
See the [scope ledger](../../CLEANUP_362_SCOPE.md) for supported operations and limitations.

## Local checks

| Check | Result |
| --- | --- |
| cargo check | Passed |
| cargo test | 1,231 passed; 5 existing export tests ignored |
| just lint-rust | Formatting and Clippy passed |
| just check-architecture | Passed |
| just generate-bindings | Three export tests passed |
| pnpm check | No errors or warnings |
| pnpm test -- --run | 444 passed in 47 files |
| pnpm build | Passed, including Tailwind verification |
| just check-version | Synchronized 0.3.81 |
| just build-fast | macOS debug application bundle built |

Focused reviewed-cache and Podcasts tests and Rust lint passed again after two
fixture-only path portability corrections. Windows execution remains for CI;
these local results do not establish Windows runtime behavior.

The bundle reports 0.3.81 in both version fields, and its executable matches the
built debug executable. Its packaged icon matches the source icon, SHA-256
`6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.
The installed/running application was not replaced. No release or tag was published.

## Actual owner commands in disposable homes

| Tool | Tested result |
| --- | --- |
| Conda 26.5.3 (Miniforge 26.5.3-0) | 118 candidates; 69,951,488 allocated bytes removed; postcheck zero |
| mise 2026.9.15 | Three roots; 24,576 allocated bytes removed; postcheck zero |
| Apple SwiftPM, Swift 6.4.0-dev | Three scopes; 24,576 allocated bytes removed; postcheck zero |

Conda 26.7.2 and mise 2026.9.16 were validated in the preceding PR. Together these
form the verified version set, not a guarantee for all intervening releases.
Tests used the production adapters with injected idle process probes. They
preserved executable/configuration/trust/installed-tool sentinels. SwiftPM also
preserved artifacts, prebuilts, configuration and security sentinels. Reproduce
that isolated command exercise with `cargo run -p neati-desktop --example validate_swiftpm_cleanup`.
No actual user cache or home Trash was cleaned.

Downloaded asset SHA-256 values:

- mise: `ec1edd2d2644e737a3aef3eb549fcac02b42236827787e7183d9a9fe11583c97`
- Miniforge: `0d765919d3ccfd1f89147aa1cf8133bfc55b3a3c13f5bacdcc091c33132fddd2`

Fixture regression tests cover scope and identity replacement, active/unknown
owners and handles, retained state, updater references, internal versus escaping
links, failed Trash moves, native home-Trash snapshot changes and cancellation.
Removed allocated bytes are command outcomes, not measured free-space recovery.

## Visual and interaction evidence

Production browser preview with deterministic mock data. All six captures were
visually inspected. The shared dark-theme class was applied explicitly for dark
captures. Native glass and actual macOS permission prompts were not visually tested.
Review opens the dialog; preview mutation is refused visibly; Escape closes it;
focus returns to Review Trash after the background scan settles.

| Size | Light | Dark |
| --- | --- | --- |
| 800 × 560 | [Light](trash-light-800x560.png) | [Dark](trash-dark-800x560.png) |
| 960 × 660 | [Light](trash-light-960x660.png) | [Dark](trash-dark-960x660.png) |
| 1280 × 800 | [Light](trash-light-1280x800.png) | [Dark](trash-dark-1280x800.png) |

## Remaining platform limits

Privileged system mutation is blocked by the unsigned distribution and missing
verified helper, as approved by the owner. Home Trash supports macOS home Trash
only. SwiftPM accepts only the validated version banner. Advisory stores remain
unavailable with documented reasons; see the scope ledger. CI results are reported
separately in the PR and are not implied by these local checks.
