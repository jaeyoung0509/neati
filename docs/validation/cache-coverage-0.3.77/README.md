# Cache coverage validation

Version: 0.3.77. Date: 2026-09-29. Host: macOS 27.0, build 26A428,
Apple silicon. Browser preview uses deterministic mock data. No real user
cache was deleted and the installed/running application was not replaced.

## Screens inspected

- [960 × 660, light, System results](system-light-960.png): selected Apple
  Python payload, advisory temporary workspace, unavailable application bundle
  and partial-scan notice are visible together.
- [960 × 660, light, Settings](settings-light-960.png): updated policy wraps
  inside the existing card.
- [800 × 560, dark, Settings](settings-dark-800.png): minimum supported window
  keeps the policy readable without horizontal overflow.
- [1440 × 900, dark, reduced motion](settings-dark-1440-reduced-motion.png):
  expanded layout and reduced-motion preference verified in the browser.

These screenshots do not establish native glass compositing or Windows runtime
behavior. Native material, icons and layout tokens were unchanged.

## Fixture evidence

The Rust coverage suite uses synthetic homes and temporary files. Each payload
is 8 KiB on this host; expected allocated sizes are read from the fixture so the
same checks work on Windows filesystems.

| Fixture | Observed | Cleanable | Result |
| --- | --- | --- | --- |
| Fresh Apple Python payload | 1 payload | 1 payload | Selected without age delay |
| Parent with E5RT model + plain payload | 2 payloads | 1 payload | Model remains protected |
| E5RT store | 1 payload | 0 | Visible, unselected |
| Plain payload + model + credential + DB/WAL + settings | 6 payloads | 1 payload | Only plain payload removed |
| Fresh named developer-tool payload | 1 payload | 1 payload | Selected when idle |
| Installed Codex runtime | 1 payload | 0 | Advisory |
| Owner starts after planning | 1 payload | No mutation | In-use refusal; file survives |

The suite also checks unknown process state, bundle-name boundary lookalikes,
exact developer roots, and reauthorization of explicit/prefix exclusions.

## Local checks

Passed: `cargo check`, `cargo test`, the six cache coverage regressions,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`pnpm check`, `pnpm test -- --run` (434 tests), `pnpm build`,
`just generate-bindings`, `just check-architecture`, `just check-version`,
and `just build-fast`. Existing explicitly ignored export/platform tests
retain their status; the binding and golden exporters were run separately.

The final `.app` declares 0.3.77 in both version fields. Its packaged
`icon.icns` is byte-identical to the tracked source. GitHub CI status is
reported on the PR separately from these local results.
