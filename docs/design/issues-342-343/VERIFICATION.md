# neati spelling, usage and navigation verification

## CI follow-up: WinGet path casing

The first CI run (36392601251) passed both platform Rust jobs, both MSRV
checks, the supply-chain audit and binding drift checks. Frontend tests failed
because the WinGet fixture looked for `neati/` while the generator correctly
retained `Neati/` for the stable `jaeyoung0509.Neati` package identifier.
Package smoke jobs were skipped, not passed.

The test now checks the exact directory entry spelling before reading files.
This assertion reproduced the mismatch on macOS as well, preventing a
case-insensitive filesystem from concealing it. The lookup matches the stable
identifier; explicit assertions keep public PackageName/DisplayName lowercase
`neati`. No shipped code, artifact naming or package identity changed in this
follow-up, and the same PR keeps its single 0.3.75 version bump.

Follow-up local checks passed: `cargo check --workspace`, `cargo test --workspace`,
`pnpm check` (zero errors/warnings), all 431 frontend tests, `pnpm build`,
`just check-version`, and `just build-fast`. The rebuilt bundle reports 0.3.75
and icon.icns. It was not installed or launched for this test-only correction.

Date: 2026-09-28. Version: 0.3.74 → 0.3.75. Host: macOS 27.0
(26A428), Apple Silicon. Issues: #342 and #343; additional owner requests
cover warm-window Quick Panel navigation and always-included extended scanning.

## Changes and boundaries

- Public spelling is `neati`, including LICENSE and documentation. Approved
  artwork geometry, handwriting and glass are unchanged. Technical identities
  (credential targets, existing log locations and WinGet package ID) are not
  display names and remain stable. The private desktop binary remains `Neati`,
  inside `neati.app`, not on PATH; #335 owns the proposed `neati` CLI command.
- Codex initialization is acknowledged before dependent requests. Required
  replies fail explicitly on timeout, malformed data, EOF and RPC errors.
  The deadline, line size and message count are bounded. Missing percentages
  never become zero; the Codex bucket is chosen explicitly.
- Verified quota is published before optional token-history collection. Other
  providers continue independently. Disabled quota rows are filtered by both
  account and panel preferences; settings revisions reject obsolete progress.
- Warm-window navigation wakes the mounted dashboard and consumes the backend
  one-shot mailbox. Cold initialization subscribes before draining it.
- Extended cache scanning is standard, including old settings with the former
  toggle disabled. Age/process/scope/structured-state checks and provider review
  remain intact. No cleanup is triggered by scanning or by these tests.

## Executed verification

- `cargo check --workspace --all-targets`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, including doctests. An earlier overlapping
  bundle/test build produced stale crate-artifact errors in rustdoc; a sequential
  complete rerun passed. No test was disabled to resolve this.
- `pnpm check`: zero errors and warnings.
- Frontend suite: 45 files, 431 tests passed.
- `just generate-bindings`, `just check-version`, `just check-architecture`:
  passed. All manifests are synchronized at 0.3.75.
- Icon generation/check: 61 outputs verified. Installer fixture tests passed.
- `just build-fast`: passed after the final runtime changes, including the
  production frontend build. Bundle metadata reports `neati`, 0.3.75 and icon.icns.

## Visual and native evidence

Browser-only mocked screenshots: [Settings](settings-light.png),
[extended scope](cache-scope-light.png), [Quick Panel](quick-light.png).
These are layout evidence, not live account or performance measurements.

With explicit permission, the installed 0.3.74 app was normally quit and the
uninstalled build-tree 0.3.75 bundle launched. Native accessibility inspection
confirmed lowercase title/menu/footer, real Codex and Antigravity usage, disabled
account preferences preserved, and the final always-included scope notice with
no intensive-cleanup checkbox. No cache deletion or installation replacement
was performed. No frontend errors were shown in the inspected Settings view.
After verification the test bundle was quit and the original installed 0.3.74
app reopened; the running installed application was not upgraded.

## Limits and follow-up

- Native tray-to-tab end-to-end automation was not completed: the automation
  surface did not expose the tray icon. Cold/warm mailbox behavior and routing
  are regression-tested; Windows and native tray acceptance still need CI/manual
  verification. Local success is not a claim that CI passed.
- No before/after latency benchmark is claimed. The new protocol test covers
  quota publication despite optional-history timeout; provider/store tests cover
  bounded independent collection and progress handling.
- The installed Codex CLI was 0.157.1. Its generated schema and the official
  [initialization contract](https://learn.chatgpt.com/docs/app-server) were checked.
  Account data lacks a stable account/workspace identifier suitable for verified
  cross-refresh stale reuse. This implementation therefore reports unavailability
  instead of reusing a potentially different account's old quota. Raw account
  responses, emails, tokens and credential files were not recorded.
- Per-phase runtime CLI-version diagnostics and an instrumented reproduction of
  the owner's original transient failure remain follow-up work. The source-level
  defects are fixed; the precise original runtime trigger is not asserted.
