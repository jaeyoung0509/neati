# Owner cleanup gap evidence — 0.3.67

This record covers issues #294, #295, #317, and #318. The live observations
were read-only. No browser cache, Homebrew candidate, or other user data was
deleted.

## Implemented contracts

- `chromium.component_downloads` recognizes the fixed macOS and Windows user
  data roots of Chrome, Brave, Edge, Chromium, Vivaldi, and Arc. One complete
  `component_crx_cache` is one unit because Chromium couples its payloads to a
  metadata index and warns against concurrent owners. The exact browser must be
  stopped; the store moves to Trash and installed components, extensions,
  models, credentials, and profile databases remain outside.
- `chromium.offline_cache_storage` enumerates only `Default`, `Profile <n>`,
  Guest, and System profiles directly below a supported browser root. It moves
  only `Service Worker/CacheStorage`, requires confirmation, and states the
  offline/redownload consequence. Cookies, passwords, history, Local Storage,
  IndexedDB, sessions, and Service Worker registration data remain siblings.
- `homebrew.cleanup` resolves a trusted standard `brew`, binds its version and
  exact `cleanup --dry-run --prune=30` candidate set into a one-shot plan,
  repeats the preview immediately before `cleanup --prune=30`, and refuses
  changed output. It is separate from the confirmed deep-download purge.

Primary ownership references:

- [Chromium CrxCache contract](https://chromium.googlesource.com/chromium/src/+/HEAD/components/update_client/crx_cache.h)
- [Chromium CacheStorage layout](https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/cache_storage/)
- [Homebrew cleanup command](https://docs.brew.sh/Manpage#cleanup-options-formula-cask-)

## Read-only live scan

The final 0.3.67 native catalog scan ran on 2026-09-27 with no exclusions and
the private path ledger kept outside the repository. Only aggregate facts are
recorded here.

| Signature | Observed | Cleanable | Selected | Result |
| --- | ---: | ---: | ---: | --- |
| Chromium component downloads | 344,117,248 B across 2 units | 181,391,360 B | 181,391,360 B | One idle-browser store automatic; one open-Chrome store blocked |
| Chromium offline CacheStorage | 948,011,008 B across 7 units | 325,828,608 B | 0 B | Idle-browser unit reviewable; open-Chrome units blocked |
| Homebrew reviewed cleanup | 164,200,000 B across 1 command unit | 164,200,000 B | 0 B | Explicit confirmation required |
| Homebrew deep downloads | 0 B on this later snapshot | 0 B | 0 B | No direct download files remained |

The whole scan reported 6,014,172,736 observed bytes, 2,524,048,960 cleanable
bytes, 1,862,631,424 selected bytes, and partial quality from unrelated access
gaps. Per-signature elapsed time was 23 ms for component stores, 133 ms for
CacheStorage, and 3,668 ms for Homebrew's owner dry-run. These are one warm,
changing-machine observation, not a performance guarantee or a claim of total
parity with Mole or Cleaner One.

## Fixture verification

Rust fixtures cover supported browser layouts, multiple profiles, an active or
unknown owner, symlinks, malformed/opaque indexes kept inside whole-store units,
identity replacement after review, preserved profile siblings, Homebrew output
parsing, untrusted roots, candidate drift, fixed command execution, and post-run
verification. Browser mutations use a fixture Trash backend; Homebrew tests use
an injected command runner and never invoke cleanup on a real installation.

The cleanup executor already records the operation-level disk-free delta
separately from provider-reported reclaimed bytes. A real live cleanup was not
performed, so this record makes no actual-reclaimed claim. The built bundle was
not installed over the currently running app.

## Verification

- `cargo check`: passed.
- `cargo test`: passed, including 857 desktop library tests plus workspace and
  integration suites.
- `just check-architecture`: passed for `zenith-core` and `zenith-platform`.
- `pnpm check`: passed with zero warnings.
- `pnpm test -- --run`: passed, 407 tests in 42 files.
- `pnpm build`: passed.
- `just check-version`: all manifests report 0.3.67.
- `just build-fast`: passed. The debug `Zenith.app` reports 0.3.67, names
  `icon.icns`, and its packaged icon SHA-256 matches the source icon.

Windows runtime behavior remains unverified locally and is left to PR CI. No
release, tag, installation, live cleanup, or merge was performed.
