# Typed inspection diagnostics — #362

Version: **0.3.83 → 0.3.84**. Local checks ran on September 30, 2026,
macOS 27.0.1 (26A434). This report covers source tests and packaging, not a
native permission grant/revoke or upgrade run.

## Behavior and scope

The source reports the inspection kind alongside diagnostic prose. Filesystem
measurements and selector expansion preserve the OS error kind at the failed
path; traversal cancellation, depth limits and safety protection are explicit.
Owner observations preserve tool discovery, unsupported status and unknown
process state. Command stderr cannot establish a filesystem permission cause.
Legacy or untyped failures remain unknown.

Only an actual access refusal at a protected macOS path offers Full Disk Access
as a possible remedy. TCC, ownership and ACLs can return the same errno, so the
app does not claim a definitive diagnosis. Other access denials offer file
permission guidance. The new optional field is diagnostic only: plans,
eligibility, identities, process guards and execution checks remain authoritative.
A missing CocoaPods owner retains observed cache bytes in a blocked row.

## Local verification

- `cargo check --workspace --offline`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo fmt --all --check`, `just check-architecture`, `just check-version`:
  passed.
- Unfiltered `cargo test --workspace --offline --no-fail-fast` was attempted.
  Three existing native tests cannot run in this sandbox: OpenRouter's loopback
  listener, the doctor CLI's real user-log write, and the controlled developer
  port helper's loopback listener. Their tests and CI execution remain intact.
- The final remaining workspace run passed **1,245 tests**, with five existing
  ignored tests and exactly those three named tests filtered locally. Regression
  coverage includes misleading/localized prose, actual OS access verdicts,
  pooled/inline depth limits, cancellation, optional-field serialization,
  unchanged cleanup authority, provider projection and retained CocoaPods bytes.
- `pnpm check`: zero errors and warnings.
- `pnpm test -- --run`: **460 passing tests across 48 files**. Storage rendering
  tests reject privacy setup for tool, unsupported, unknown and ordinary access
  failures even when diagnostic prose mentions Full Disk Access.
- TypeScript bindings were generated through the ignored export test.
- `pnpm build`: passed; production Tailwind utility verification passed.
- `pnpm icons:check`: passed, 61 assets/registry entries.
- `just build-fast`: passed. Both bundle version fields are 0.3.84; the Neati
  executable is present and the packaged ICNS matches the tracked source.
  The frontend was rebuilt and embedded through the default custom protocol.

## Visual evidence

[Tool/unsupported/unknown example](tools.html) and
[protected-path access example](privacy.html) are self-contained HTML snapshots
rendered by the actual Svelte Storage view with the production CSS. They contain
synthetic scan facts and static controls; they do not exercise IPC or TCC.

Chrome computer use was rejected by automatic approval review
(`Computer Use was not approved to use Google Chrome`). No browser screenshot
or native visual run was completed inside this session. The terminal upload
script captures these fixtures in a fresh, isolated headless browser and adds
`tools.png` and `privacy.png` in a documentation-only commit before publishing.
Its page-text checks require the expected typed labels and privacy routing.
Those browser fixtures do not establish native glass, permission changes or
interactive access-recheck behavior.

## Remaining validation

Unsigned-bundle permission denial, grant, revocation, relaunch and upgrade remain
unverified. Installed provider/toolchain compatibility is not expanded here.
Windows native behavior awaits its runner. Paid signing and privileged-helper
cleanup remain deferred. No real user caches were cleaned, and the installed or
running app was not replaced. CI status must be reviewed separately on the PR.
