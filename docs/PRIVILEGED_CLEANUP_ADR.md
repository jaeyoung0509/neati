# Privileged cleanup design

Status: design accepted for implementation planning; distribution blocked.
Issue: #362. Decision recorded September 29, 2026.

The owner approved proceeding with ordinary user operations and retaining
privileged system cleanup as a blocked item in #362. No privileged helper is
installed, registered, launched or advertised as available by this change.

## Decision

Use a separately signed, bundled LaunchDaemon registered through SMAppService
on supported macOS releases. Require explicit system approval. Authenticate
both endpoints by their code-signing identity and verify the client audit token;
a bundle name, process name or file path alone is not authentication. Bind the
helper and client to the approved publisher and protocol version. An unsigned
or mismatched client must receive an unavailable/refused result.

The helper accepts a closed set of operation identifiers and backend-minted,
expiring, one-shot inventory/plan IDs. It accepts no arbitrary path, shell text,
command, UID, environment or executable. It independently resolves each exact
system scope, produces a bounded preview, rechecks file identities and use at
execution, supports cancellation and reports per-item outcomes. The application
operation gate still serializes its own storage mutations. Helper restart,
client exit and expired/replayed plans revoke authorization.

First candidate operations are specific aged diagnostic/log payloads under
/Library/Caches, DiagnosticReports and /private/var/log. Each needs a separate
lifecycle contract, owner check, retention rationale and adapter fixture before
it becomes allowlisted. DiagnosticPipeline and powerlog are separate research
items. Software Update stores, whole databases, models, backups and VM disks
are not targets. Neither sudo nor AuthorizationExecuteWithPrivileges nor an
AppleScript shell prompt is a fallback.

## Blocking prerequisites

CODE_SIGNING_POLICY.md explicitly records unsigned, unnotarized macOS beta
artifacts and no Developer ID certificate. Before shipping a helper, establish
publisher signing, notarization, reproducible helper packaging and signed
end-to-end install/update/removal tests. Test client authentication, user denial,
helper absence/version skew, tampered binary, cancellation, timeout, inaccessible
roots and source replacement. Record the macOS build and installed identities.
None of those prerequisites is implied by the user's Full Disk Access grant.

The system cache/log signatures remain observation-only with an explanation;
this document does not complete the system-mutation checklist item.

Reference: [Apple SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice)
registers bundled helpers subject to user approval on macOS 13 and later.
