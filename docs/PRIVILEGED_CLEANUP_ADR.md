# Privileged cleanup design

Status: future design retained; privileged implementation deferred by owner.
Issue: #362. Initial decision September 29; revised September 30, 2026.

The owner has no near-term plan to buy an Apple Developer membership or adopt
Developer ID signing/notarization. User-level cleanup and permission guidance
proceed independently of that decision. Paid signing and privileged system
mutation are outside the current #362 completion criteria; they resume only
on an explicit owner decision. No privileged helper is installed, registered,
launched or advertised as available.

Full Disk Access is a user-controlled macOS privacy grant. It does not override
file ownership, ACLs, SIP or application deletion safeguards. The existing
settings-navigation command is reused; returning to the app triggers a fresh
scan of actual locations, never an assumed global grant. A privacy access gap
is a possible cause of denial, not proof that filesystem permissions allow
mutation. A new user-accessible system owner operation still needs exact scope,
lifecycle evidence, a reviewed catalog-policy contract and regression tests.
See [the system access assessment](SYSTEM_CLEANUP_ACCESS.md).

## Future privileged-helper decision

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

## Prerequisites before resuming privileged work

CODE_SIGNING_POLICY.md explicitly records unsigned, unnotarized macOS beta
artifacts and no Developer ID certificate. Before shipping a helper, establish
publisher signing, notarization, reproducible helper packaging and signed
end-to-end install/update/removal tests. Test client authentication, user denial,
helper absence/version skew, tampered binary, cancellation, timeout, inaccessible
roots and source replacement. Record the macOS build and installed identities.
None of those prerequisites is implied by the user's Full Disk Access grant.

The system cache/log signatures remain observation-only with an explanation;
this document does not claim an implemented system-mutation adapter.

Reference: [Apple SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice)
registers bundled helpers subject to user approval on macOS 13 and later.
