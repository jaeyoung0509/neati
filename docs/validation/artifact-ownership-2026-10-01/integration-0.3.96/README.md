# Ordered PR integration at 0.3.96

Validation date: October 1, 2026. Source checkpoint:
`876bea5404fba7d336347f04d8bbfc9034fca981`. Host: macOS 27.0.1 (26A434), arm64.

The owner explicitly authorized merging both open PRs without waiting for
Windows packaging. [#398](https://github.com/jaeyoung0509/neati/pull/398) merged
first, advancing develop from **0.3.94 to 0.3.95** at
`f03f42ef1fe66b97fc7bc0561499fc56ef837373`. [#399](https://github.com/jaeyoung0509/neati/pull/399)
merged that target into its feature branch without conflicts and used
`just bump-patch` to synchronize its final **0.3.95 → 0.3.96** transition.
This resolves the previously shared patch proposal using the required recipes.

#394's exact-empty XDG source and fixtures retain their merged behavior.
#390's authored-content protection and #391's framework observation-only rules
are unchanged by integration. Framework removal, build/dev owner-use and custom
output ownership remain open under #391. No dependency was added by integration.
A second agent reviewed the combined source without running builds or tests.

## Final local verification

[local-checks.json](local-checks.json) records fresh successful frontend
typecheck, **485 frontend tests**, production build, format, workspace check,
all-targets Clippy with denied warnings, **1,343 Rust tests**, architecture,
binding generation with no drift, version/icon checks and `just build-fast`.
Six existing default ignored Rust tests remain unchanged; none was filtered.
Full-log hashes and the executable digest are recorded; logs remain in the
durable recovery archive.

Both bundle versions are **0.3.96** and the packaged icon matches its source.
The executable passed [all 14 doctor checks](doctor.json) and embeds
[all 20 current production assets byte-for-byte](embedded-assets.json).
Its embedded HTML references the current `./assets/index-Cn6B8yri.js`.

## CI and visual evidence

#398's nine PR checks passed before its merge. The preceding #399 source at
`88f71f3` passed native macOS/Windows Rust, both MSRV jobs, shared frontend,
supply-chain/security and macOS packaging; its Windows packaging result was
not awaited. The [native Windows fixture proof](https://github.com/jaeyoung0509/neati/actions/runs/36843476845)
includes the exact malformed/missing metadata cases, linked worktree,
identity/content stamp regressions and restored-time replacement refusal.
The final integration head's remote CI is reported separately in the PR;
no pending check is recorded as successful.

UI source, icons and design tokens are unchanged from `88f71f3`; the application
version advances to .96. The earlier **43 browser checks and seven screenshots**
remain .95 interaction evidence, rather than a new .96 native visual run.
Interactive native Windows/Linux, tray/glass behavior and GUI cleanup remain
unverified. No real-user cleanup, installation or GUI launch ran.
