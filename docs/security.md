# Threat model and defensive boundaries

## Assets and adversaries

Protected assets include user files outside grants, credentials, session tickets, typed
text/clipboard contents, screen artifacts, authorization state and the integrity of the
chosen target. Expected failures include ambiguous controls, disappearing objects, prompt
injection in observed content, interrupted mutations, malicious plugin payloads, path
traversal, symlink races, spoofed local endpoints and excessive output/resource use.

The broker is trusted code; MCP clients and observed application content are untrusted
intent/data. Plugins are separate, less-trusted processes. A user grants selected rights
through owner-controlled configuration and a separate operator console. The model must
not be able to supply its own affirmative confirmation.

## Implemented defenses in source

Wire input is bounded and versioned. Sockets/directories/files have owner and mode checks;
peer UIDs must match. Schema validation precedes dispatch. Exact references retain session,
backend, identity, fingerprint, generation and expiration; live validation runs again
before a side effect. A mutation with uncertain completion is not retried or rerouted.

Filesystem commands reject absolute/parent paths, symlinks, mount crossings and nonregular
or multiply linked files. `openat2` resolves under a pinned directory FD; temporary writes
use exclusive creation, `0600`, fsync and relative rename. This requires kernel support;
there is no weak string-only fallback. Initial grant setup assumes owner-controlled
canonical directories. Do not let an unrelated writer replace the configured root during
startup. Blender file/render operations cannot receive equivalent FD-relative protection
through bpy and require a separately trusted private workspace.

The plugin/driver launcher checks binary digests, stages owned executables, scrubs the
environment, provides named mounts, uses resource budgets, and requires bubblewrap plus a
Landlock helper. Missing isolation is a denial, not permission to run directly. Network
needs both manifest declaration and owner opt-in. No inherited SSH agent/session-bus/
cloud-token environment is provided. Hosted hostile plugin and DriverProvider regressions
exercise mount escape, host-file/PID/loopback isolation, environment scrubbing, resource
limits and descendant cleanup. Plugin Protocol v2 also attests child version and complete
command-descriptor digest. These regressions reduce known boundary risk but are not a
formal proof of the kernel/Bubblewrap/Landlock/native-code stack.

Audit stores typed metadata, not input/output bodies. It rotates bounded local files and
chains hashes to detect accidental corruption. A same-UID attacker can rewrite it, so it
is not an external tamper-proof ledger. Errors redact backend body text. The inspector
escapes terminal control characters. Portal and browser screenshots are temporary private
artifacts; normal expiry handling exists, but crash-recovery cleanup is not yet complete.

## Residual risks and required review

X11 blocking work now crosses a bounded blocking boundary and its refs carry lifecycle epochs,
but the full native-desktop matrix remains incomplete. Some operation-level capability discovery
is still coarse. Configuration changes require restart; there is no independently authenticated
multi-principal policy service. Recipe taint redaction is conservative but not a formal noninterference guarantee.
Schema and Chromium quota/artifact-lifecycle evidence must be evaluated at its recorded SHA;
older unresolved-status prose is not a replacement for the R09/R14 disposition. Existing apps can have
side effects outside a broker filesystem grant because the apps themselves are not
sandboxed. A declared action being accepted does not prove the UI has reached the intended
postcondition; use a fresh observation and assertions.

Test priorities: stale identity reuse, focus drift, consent revocation during input,
partial mutation on disconnect, duplicate JSON keys, same-UID MCP-upstream isolation,
output flooding, audit failure before/after effect, poisoned text in every rendering
surface, artifact cleanup after abnormal termination, sandbox-kernel variation and
configuration TOCTOU boundaries.
Independent security review remains a separate release-readiness requirement.

## Platform-specific enforcement

Platformization does not reduce Linux enforcement to a portable lowest common denominator.
Linux filesystem access continues to use pinned-directory/openat2 semantics and Linux
driver/plugin execution continues to require its sandbox path.

The macOS host uses public Apple APIs and treats TCC as an external user-consent boundary.
Accessibility, input and screen capture must never be enabled by modifying TCC databases,
disabling SIP or using private entitlements. App Sandbox is not represented as an equivalent to
Linux bubblewrap/Landlock for an accessibility host.

The macOS Driver/Plugin Host therefore fails closed where arbitrary third-party executable
isolation has not been proven with a supported Apple mechanism. Digest/Mach-O validation is an
identity check, not a sandbox. Native CI can establish compilation/linking and noninteractive
tests; Accessibility/Input/Screen Recording acceptance requires a real authorized Mac session.

## Composition, persistent knowledge and review status

Prepared plans, stored Graph identities, Skills, provider metadata and Effects reports are
not additional permission grants. Broker policy and per-operation provenance remain the
authority path. A prepared plan is an owner-bound, bounded attempt, not blanket consent;
a persisted asset identity is not a current live reference. Provider-scoped application
sessions and user-session-scoped jobs have different lifetime/privacy boundaries.

Treat readback according to its declared scope and completeness. UNKNOWN, incomplete
enumeration or an uncertain action outcome must not be promoted to global success.
Private project/Graph state and evidence may contain application information even though
the audit ledger records metadata. Apply owner-controlled storage and retention policies;
there is no claim that a metadata audit makes all application state non-sensitive.

R's review records source and suite identities separately in
[the evidence directory](../verification/r16-closeout/README.md). It is an AI-assisted
repository review, not an external security audit. Direct source observations, historical
certificates, fresh checks and unexecuted requirements are distinct. Any R-authored
security-relevant documentation or verification change needs separate review before
being relied on for release. R16 is CLOSED under its recorded separate revalidation. Residual
physical/interactive cases remain OPEN/deferred post-v1; the independent public-release security
review remains mandatory and pending. See [release policy](release-policy.md).
