# Storage access setup validation

Issue #362, version 0.3.81 → 0.3.82. September 30, 2026.
Host: Apple Silicon, macOS 27.0.1 (26A434).

This PR delivers permission guidance/recovery first. Additional owner adapters,
compatibility expansion and native TCC grant/revoke/update validation remain in
#362. Privileged mutation and paid signing are owner-deferred; no system cleanup
adapter is enabled. See [system access assessment](../../SYSTEM_CLEANUP_ACCESS.md).

## Local checks

| Check | Result |
| --- | --- |
| `cargo check` | Passed |
| `cargo test` | 1,237 passed, five existing export tests ignored; doc tests passed |
| `just lint-rust` | Formatting and Clippy passed |
| `just check-architecture` | Passed |
| `pnpm check` | Zero errors and warnings |
| `pnpm test -- --run` | 456 passed in 48 files |
| `pnpm build` | Passed, including Tailwind verification |
| `just check-version` | Synchronized 0.3.82 |
| `just build-fast` | Debug macOS application bundle built |
| Bundled `Neati --doctor` | 14/14 checks passed |

The initial parallel Cargo test/Tauri build run passed Rust tests but failed
rustdoc dependency linking. Running `cargo test` alone passed, including doc
tests; the app was rebuilt afterwards. This is not a skipped failing test.

Both bundle version fields report 0.3.82. The packaged icon matches
`src-tauri/icons/icon.icns`, SHA-256
`6762519752de4ffc84865d6edc7a80462f3d1edebf4d7c2d94cd9ffa1c460dc9`.
The installed/running application was not replaced. No release, tag, permission
grant or real user cleanup was performed. CI status is separate from local checks.

## Recovery behavior

Ten store tests exercise the actual recovery store through injected ports:
ordinary focus and Settings opening do not start a scan; an explicit Settings
round trip requests one scan; repeated activation is coalesced; early return
before the command completes is retained; active storage work queues the scan;
hidden queues resume on activation; subscriber disposal is reference counted
and idempotent; failures can be retried; late responses cannot revive an
unmounted surface; preview cannot dispatch native Settings or permission checks.
The existing Storage render regression verifies actual setup copy and gap counts.
Platform regression coverage retains ordinary/system path denials without a
Full Disk Access attribution, including a protected-root lookalike.

## Visual and browser interaction evidence

Vite browser preview with deterministic mocked data, not native-webview or TCC
evidence. Settings instructions were expanded for captures. Storage used a
fixture scan with three privacy-access gaps and one ordinary permission-denied
gap, retaining the preview's verified items. All twelve captures were inspected.
All sizes had no horizontal page overflow and reduced-motion media enabled.

| Size | Settings light / dark | Storage light / dark |
| --- | --- | --- |
| 800 × 560 | [Light](settings-light-800x560.png) / [Dark](settings-dark-800x560.png) | [Light](storage-light-800x560.png) / [Dark](storage-dark-800x560.png) |
| 960 × 660 | [Light](settings-light-960x660.png) / [Dark](settings-dark-960x660.png) | [Light](storage-light-960x660.png) / [Dark](storage-dark-960x660.png) |
| 1280 × 800 | [Light](settings-light-1280x800.png) / [Dark](settings-dark-1280x800.png) | [Light](storage-light-1280x800.png) / [Dark](storage-dark-1280x800.png) |

The disclosure toggles with Enter while focused. Check Access and Open System
Settings both show explicit desktop-required errors in browser preview, without
claiming that permission changed or that a native check ran.

## Unverified native behavior

macOS Full Disk Access grant, denial, revocation, relaunch and persistence across
unsigned-bundle replacement were not executed. Native focus delivery was not
validated; Check Access remains the explicit fallback. The macOS setup is gated
on backend platform context; Windows runtime behavior remains for CI/manual
verification. Browser recovery tests do not prove TCC behavior or filesystem
mutation access. A successful scan never constitutes deletion authorization.
