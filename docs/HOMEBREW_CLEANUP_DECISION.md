# Homebrew cleanup operation boundary

Neati exposes two deliberately separate Homebrew actions on macOS:

1. **Homebrew Downloads** is the deep download purge. Its owner-scoped
   filesystem adapter reviews direct, single-linked files in
   `~/Library/Caches/Homebrew/downloads`, rechecks identity and size, and then
   removes only the selected files.
2. **Homebrew Reviewed Cleanup** delegates to Homebrew's public CLI. It reviews
   the old formula versions, stale lock files, and outdated downloads returned
   by `brew cleanup --dry-run --prune=30`, then runs the matching fixed command
   only after explicit confirmation.

The two estimates may overlap and must never be added as if their target sets
were disjoint. API and bootsnap metadata and unrecognized download entries
remain advisory.

## Command contract

The published [Homebrew command reference](https://docs.brew.sh/Manpage#cleanup-options-formula-cask-)
states that `cleanup` removes stale lock files, outdated downloads, and old
installed formula versions, and that `--dry-run` shows what would be removed.
The adapter applies the following narrower contract:

- resolve only `/opt/homebrew/bin/brew` or `/usr/local/bin/brew`, require an
  ordinary executable file, and bind its filesystem identity into the private
  one-shot plan;
- read and bind the `Homebrew <version>` line;
- run only `cleanup --dry-run --prune=30` and `cleanup --prune=30`, with update,
  color, and environment hints disabled and bounded execution time;
- accept only exact absolute `Would remove:` candidates under the trusted
  Homebrew prefix or the current user's Homebrew cache, plus the aggregate size
  line; unknown stdout, a timeout, an unexpected root, or a missing size summary
  blocks the action;
- hash the version, executable, candidate paths, and aggregate estimate into
  the reviewed unit, repeat the dry-run immediately before execution, and
  refuse any change;
- require Homebrew and Ruby to be idle and require explicit confirmation, so
  neither Homebrew action enters Quick Clean;
- run no recursive fallback and never include `autoremove` or `--scrub`;
- re-run the preview afterward. The provider reports the reviewed amount minus
  the remaining preview, while the cleanup result separately records the
  whole-operation disk-free delta collected by the executor.

Homebrew's preview is human-readable rather than a transaction token. Parsing
therefore fails closed when its stdout contract changes. Fixture tests cover
exact parsing, unexpected roots, candidate drift, command failure, partial
post-verification, and process/identity refusals without running Homebrew on a
real user store. Windows and Linux have no Homebrew adapter in Neati.
