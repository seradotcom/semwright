# Motion Canvas security model

The managed format exists because arbitrary TypeScript is executable code. The first-party driver deliberately does **not** expose eval, arbitrary TS/TSX, shell commands, arbitrary npm install, arbitrary remote scripts or arbitrary easing/generator expressions.

## Trust boundaries

Agent input is untrusted semantic data. Driver Protocol descriptors are strict, digest-pinned schemas. The Rust driver owns policy-relevant validation and never delegates authorization to Node or the browser.

The Driver Host is the execution boundary. Production rendering requires Bubblewrap + Landlock, named owner grants, a pinned driver ELF, `network=false` and bounded process/file/CPU/address-space resources. There is no unsandboxed fallback.

The Motion runtime is an explicit read-only owner grant. Node, renderer helper and Chromium are each verified against SHA-256 before use. Runtime configuration parsing is strict and bounded; malformed or stale tools make rendering unavailable.

## Files and assets

The project store rejects noncanonical roots, traversal, absolute/remote paths and symlink escapes. Semantic/project/media reads are bounded. Assets record SHA-256 and byte length; copies are rechecked before generated-project use.

SVG is parsed as a restricted local structural subset: scripts, event handlers and external references are rejected. Image dimensions/decoded allocation are bounded. Code-node text, prompt-injection-looking strings, template syntax and backticks remain display data and are escaped by deterministic codegen.

Generated source is written to a driver-owned content-addressed tree; agent text is never concatenated as executable TypeScript syntax. Dependency versions are exact and the lockfile is part of deterministic output. Runtime package scripts are disabled in CI installation.

## Browser

The browser uses a disposable Playwright context, no user profile, no credentials or extensions. Built assets are fulfilled through request interception from the synthetic `semwright.invalid` origin. Other requests are aborted. No Vite server listens on loopback or LAN.

Because nested Chromium user namespaces are unavailable on the certified Ubuntu runner, Chromium's own sandbox is disabled only after the helper proves it is already inside the Driver Host's Bubblewrap + Landlock boundary. Direct helper execution fails closed.

## Jobs and artifacts

Render jobs allow at most two active jobs and a bounded retained registry. Timeout/cancel terminates the process group and removes partial output. Child stdout/stderr are bounded.

PNG artifacts must have exact expected sequential names/count, bounded decode size and planned dimensions. Transparent renders must prove at least one non-opaque pixel. Artifacts are represented by paths/hashes and are never embedded in protocol JSON.

## Known boundaries

Driver Protocol v1 does not negotiate child events or cooperative cancellation; cancellation is driver-local. Registry packages do not yet provide a generic multi-tool runtime distribution primitive. External arbitrary Motion Canvas projects are not safely mutable and are therefore not accepted as managed semantic data.
