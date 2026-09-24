# Motion Canvas security model

The managed format exists because arbitrary TypeScript is executable code. The first-party driver deliberately does **not** expose eval, arbitrary TS/TSX, shell commands, arbitrary npm install, arbitrary remote scripts or arbitrary easing/generator expressions.

## Trust boundaries

Agent input is untrusted semantic data. Driver Protocol descriptors are strict, digest-pinned schemas. The Rust driver owns policy-relevant validation and never delegates authorization to Node or the browser.

The Driver Host is the execution boundary. Production rendering requires Bubblewrap + Landlock, named owner grants, a pinned driver ELF, `network=false` and bounded process/file/CPU/address-space resources. There is no unsandboxed fallback. The final Firefox route stays within the existing 4 GiB Driver Host virtual-address-space ceiling and requests 256 tasks, the existing SDK hard maximum, after CI observed Firefox 151 WebRender thread creation fail with `EAGAIN` at 128; the SDK default remains 512 MiB.

Ubuntu 24.04 additionally restricts unprivileged user namespaces through AppArmor. CI preserves that system-wide restriction and specializes only the ephemeral distro `bwrap-userns-restrict` profile for Semwright's exact Driver Host exec chain: `/plugin/sandbox` performs the one privilege-dropping stacked transition, then `/plugin/bin`, `/workspace/**` and `/tmp/**` may only inherit (`ix`) the already-enforced confinement. Other descendant exec paths remain denied. An exact-depth probe also sets `no_new_privs` before the driver/tool execs.

The Motion runtime is an explicit read-only owner grant with `execute: true`; that executable bit is narrowly attested in the Driver manifest and is not inherited by project/media/config mounts or plugins. Fontconfig is a separate non-executable read-only grant mapped only to `/etc/fonts`, because Driver Host otherwise constructs a minimal `/etc`. Node, `render.mjs` and the Playwright-pinned Firefox executable are each verified against SHA-256 before use. Runtime configuration parsing is strict and bounded; malformed or stale tools make rendering unavailable.

## Files and assets

The project store rejects noncanonical roots, traversal, absolute/remote paths and symlink escapes. Semantic/project/media reads are bounded. Assets record SHA-256 and byte length; copies are rechecked before generated-project use.

SVG is parsed as a restricted local structural subset: scripts, event handlers and external references are rejected. Image dimensions/decoded allocation are bounded. Code-node text, prompt-injection-looking strings, template syntax and backticks remain display data and are escaped by deterministic codegen.

Generated source is written to a driver-owned content-addressed tree; agent text is never concatenated as executable TypeScript syntax. Dependency versions are exact and the lockfile is part of deterministic output. Runtime package scripts are disabled in CI installation.

## Browser

Firefox uses a disposable Playwright context/profile, no user profile, no credentials and no extensions. Built assets are fulfilled through request interception from the synthetic `semwright.invalid` origin; every other page request is aborted. No Vite server, CDP endpoint or other browser-control listener is exposed.

The helper can launch only the SHA-256-pinned Firefox path supplied by Rust. It sets fixed `MOZ_ASSUME_USER_NS=0` and `MOZ_DISABLE_CONTENT_SANDBOX=1` values because Firefox's nested content sandbox is not the authority inside the already-required Bubblewrap + Landlock boundary. It also pins `dom.ipc.forkserver.enable=false`: CI showed the Linux fork-server IPC path failing to create the tab subprocess inside the outer namespace, so Firefox falls back to its ordinary child-process launch path rather than adding another process-broker layer. The browser is started as a fresh persistent context under the job-private temporary tree and reuses its initial `about:blank` page; no owner/user Firefox profile is accessed. These values are not agent-controlled. Node and Firefox inherit one owned process group, so timeout/cancellation terminates descendants as a tree. Filesystem grants remain explicit and `network=false` remains in force.

Chrome-for-Testing/Chromium experiments are not a fallback path. Multiple pinned Chromium runs aborted with upstream-style `SIGTRAP/int3` before CDP startup; the driver does not weaken its sandbox to accommodate that browser failure.

## Jobs and artifacts

Render jobs allow at most two active jobs and a bounded retained registry. Timeout/cancel terminates the process group and removes partial output. Child stdout/stderr are bounded.

PNG artifacts must have exact expected sequential names/count, bounded decode size and planned dimensions. Transparent renders must prove at least one non-opaque pixel. Artifacts are represented by paths/hashes and are never embedded in protocol JSON.

## Known boundaries

The integrated SDK supports Driver Protocol v2, but this driver deliberately negotiates v1 in this PR and does not claim v2 child events/progress/cancellation; render cancellation is driver-local. Registry packages do not yet provide a generic multi-tool runtime distribution primitive. External arbitrary Motion Canvas projects are not safely mutable and are therefore not accepted as managed semantic data.
