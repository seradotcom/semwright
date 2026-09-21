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

The plugin launcher checks the binary digest, stages an owned executable, scrubs the
environment, provides named mounts, uses resource budgets, and requires bubblewrap plus a
Landlock helper. Missing isolation is a denial, not permission to run directly. Network
needs both manifest declaration and owner opt-in. No inherited SSH agent/session-bus/
cloud-token environment is provided. This code has not been executed against adversarial
plugins; see the release gate for negative tests and complete handshake validation.

Audit stores typed metadata, not input/output bodies. It rotates bounded local files and
chains hashes to detect accidental corruption. A same-UID attacker can rewrite it, so it
is not an external tamper-proof ledger. Errors redact backend body text. The inspector
escapes terminal control characters. Portal and browser screenshots are temporary private
artifacts; normal expiry handling exists, but crash-recovery cleanup is not yet complete.

## Residual risks and required review

X11 calls still block within async methods, and its object fingerprint is not a complete
lifecycle identity. Some operation-level capability discovery is coarse. Configuration
changes require restart; there is no independently authenticated multi-principal policy
service. Recipe taint redaction is conservative but not a formal noninterference guarantee.
Generic output schemas need tightening. CDP downloads need quotas. Existing apps can have
side effects outside a broker filesystem grant because the apps themselves are not
sandboxed. A declared action being accepted does not prove the UI has reached the intended
postcondition; use a fresh observation and assertions.

Test priorities: stale identity reuse, focus drift, consent revocation during input,
partial mutation on disconnect, duplicate JSON keys, plugin filesystem/network escape,
output flooding, audit failure before/after effect, poisoned text in every rendering
surface, artifact cleanup after abnormal termination, and configuration TOCTOU boundaries.
No independent security review has been performed in this handoff.
