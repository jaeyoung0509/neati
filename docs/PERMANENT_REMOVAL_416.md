# Permanent removal policy — issue #416

The owner explicitly approved permanent deletion for **all deletion features** on October 9, 2026, including personal files, apps, generated artifacts and reviewed temporary folders that may contain source. Version: **0.3.106 → 0.3.107**. The change alters the disposal method of eligible, authorized targets; discovery alone still grants no authority.

## Analysis

Before this change, `DeleteTarget::execution_mode` inferred permanent removal for Safe and Trash for Rebuild. Chromium, DotSlash and reviewed cache owners used Trash. Large Files, App Uninstaller, Developer Artifacts and Temporary Folders also used Trash. Trash moves did not release storage until emptied.

Mole was inspected read-only: installed Homebrew **1.55.0**, and upstream revision `2e8ea7f3aa8c20517e3597cc437bec1030e8401b`. Its [safe_remove implementation](https://github.com/tw93/Mole/blob/2e8ea7f3aa8c20517e3597cc437bec1030e8401b/lib/core/file_ops.sh) validates the target before `rm -rf`; [uninstall](https://github.com/tw93/Mole/blob/2e8ea7f3aa8c20517e3597cc437bec1030e8401b/bin/uninstall.sh) defaults to Trash and offers `--permanent`. `mo analyze` also uses Trash. Mole therefore supplied a useful validation reference, while the requested all-permanent policy is broader than Mole's defaults.

| Workflow | Previous disposal | Current disposal |
| --- | --- | --- |
| Ordinary Safe / Rebuild filesystem cleanup | Permanent / Trash | Permanent protected-entry removal |
| Chromium, DotSlash, reviewed cache owners | Trash | Identity-bound permanent removal of each authorized unit |
| Large Files / App Uninstaller | Trash | Reviewed permanent removal; failed app bundle removal still blocks related data |
| Developer Artifacts / generated framework units | Trash | Reviewed permanent removal with fresh ownership, Git, metadata and use checks |
| Explicit Temporary Folders review | Trash | Staged permanent removal; source-loss and unknown-use consent retained |
| Home Trash / Windows Recycle Bin emptying | Permanent | Existing permanent typed adapter |
| Models, Docker and fixed-argument tool providers | Owner/runtime command | Existing permanent owner/runtime command; no filesystem fallback |

## Implementation and limits

No shell deletion command or arbitrary-path IPC was added. Backend-owned expiring one-shot plans bind exact IDs, scope and identity. Catalog entries requesting Trash are rejected. Historical Trash/mixed DTO values and explicit compatibility fixtures remain readable, but no current catalog target or shipped owner adapter chooses Trash.

Ordinary cleanup retains its protected-state `SafeTreeDeleter`. Reviewed whole units use `RemovalBackend`: Unix traverses verified no-follow parent descriptors and unlinks inspected entries; Windows pins ancestors and the inspected tree without delete-sharing, refuses reparse points and removes through handles. Fresh workflow checks run after native inspection and before the first mutation. Limits are 30 seconds, depth 128, 100,000 Unix entries or 4,096 Windows inspected unit handles. Oversized or unsupported units are refused rather than sent to a fallback. Existing cancellation admission and between-unit checks remain; native recursive removal does not add an interrupt at every entry.

Permanent deletion is not secure erasure, an atomic transaction, or a promise of immediately recovered physical space. Unix verifies identities immediately before descriptor-relative unlink, but does not provide an atomic compare-and-unlink primitive against a concurrent same-user adversary. Errors can occur after some entries were removed. Those errors explicitly report possible partial removal; only remaining staged entries can be restored, and successful removed-byte estimates remain separate from measured free-space changes.

## Verification evidence

All mutation tests use disposable fixtures. No real user caches, personal files, applications, source trees or Trash were deleted during validation. Local macOS checks and browser evidence are recorded in [the validation report](validation/permanent-cleanup-0.3.107/README.md). Windows native execution is deferred to Windows CI/runtime validation; cross-compilation alone is not platform parity evidence. The built debug app was not installed or launched, and the running app was not replaced.
