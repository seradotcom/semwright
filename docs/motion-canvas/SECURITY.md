# Motion Canvas security model

The managed format exists because arbitrary TypeScript is executable code. The first-party driver deliberately does **not** expose eval, arbitrary TS/TSX, shell commands, arbitrary npm install, arbitrary remote scripts or arbitrary easing/generator expressions.

## Trust boundaries

Agent input is untrusted semantic data. Driver Protocol descriptors are strict, digest-pinned schemas. The Rust driver owns policy-relevant validation and never delegates authorization to Node or the browser.

The Driver Host is the execution boundary. Production rendering requires Bubblewrap + Landlock, named owner grants, a pinned driver ELF, `network=false` and bounded process/file/CPU/address-space resources. There is no unsandboxed fallback. The final Firefox route stays within the existing 4 GiB Driver Host virtual-address-space ceiling and requests the existing SDK maximum of 256 tasks; the SDK address-space default remains 512 MiB.

Ubuntu 24.04 additionally restricts unprivileged user namespaces through AppArmor. CI preserves that system-wide restriction and specializes only the ephemeral distro `bwrap-userns-restrict` profile for Semwright's exact Driver Host exec chain: `/plugin/sandbox` performs the one privilege-dropping stacked transition, then `/plugin/bin`, `/workspace/**` and `/tmp/**` may only inherit (`ix`) the already-enforced confinement. Other descendant exec paths remain denied. An exact-depth probe also sets `no_new_privs` before the driver/tool execs.

The Motion runtime is an explicit read-only owner grant with `execute: true`; project/media/output/fontconfig grants do not inherit that execute authority. Node is a separate SHA-pinned `Manifest.tools` entry whose Host child receives only the declared `project`, `output`, `runtime` and `fontconfig` mounts. `render.mjs` is embedded in the Rust driver and delivered over bounded stdin rather than loaded from a mutable runtime path. The Playwright/Firefox resource tree remains inside the owner-granted executable runtime bundle; the helper canonicalizes the selected Firefox executable and rejects it if it escapes that bundle. Full bundle immutability is not claimed.

## Files and assets

The project store rejects noncanonical roots, traversal, absolute/remote paths and symlink escapes. Semantic/project/media reads are bounded. Assets record SHA-256 and byte length; copies are rechecked before generated-project use.

SVG is parsed as a restricted local structural subset: scripts, event handlers and external references are rejected. Image dimensions/decoded allocation are bounded. Code-node text, prompt-injection-looking strings, template syntax and backticks remain display data and are escaped by deterministic codegen.

Generated source is written to a driver-owned content-addressed tree; agent text is never concatenated as executable TypeScript syntax. Dependency versions are exact and the lockfile is part of deterministic output. Runtime package scripts are disabled in CI installation.

## Browser

Firefox uses a disposable Playwright context/profile, no user profile, no credentials and no extensions. Built assets are fulfilled through request interception from the synthetic `semwright.invalid` origin; every other page request is aborted. No Vite server, CDP endpoint or other browser-control listener is exposed.

The helper can launch only the Playwright-selected Firefox executable whose canonical path remains inside the delegated runtime bundle. It sets fixed `MOZ_ASSUME_USER_NS=0` and `MOZ_DISABLE_CONTENT_SANDBOX=1` values because Firefox's nested content sandbox is not the authority inside the already-required Bubblewrap + Landlock boundary. It also pins `dom.ipc.forkserver.enable=false`: CI showed the Linux fork-server IPC path failing to create the tab subprocess inside the outer namespace, so Firefox falls back to its ordinary child-process launch path rather than adding another process-broker layer. The browser is started as a fresh persistent context with writable state redirected to the job output root; no owner/user Firefox profile is accessed. These values are not agent-controlled. Node and Firefox are descendants of one Host-owned runtime-tool job, so cancellation, timeout and provider shutdown reap the tree. Filesystem grants remain explicit and `network=false` remains in force.

Chrome-for-Testing/Chromium experiments are not a fallback path. Multiple pinned Chromium runs aborted with upstream-style `SIGTRAP/int3` before CDP startup; the driver does not weaken its sandbox to accommodate that browser failure.

## Jobs and artifacts

Render jobs allow at most two active semantic jobs and a bounded retained registry. The executable work is a session-bound Host runtime-tool job; timeout, explicit cancel, provider shutdown and parent-request cleanup cancellation converge on Host reaping. Tool output is bounded before the driver accepts a terminal result.

PNG artifacts must have exact expected sequential names/count, bounded byte size and planned dimensions. Every frame receives compressed-byte SHA-256 plus bounded PNG header validation; short renders are pixel-decoded exhaustively and long renders use deterministic deep pixel samples. Transparent renders must prove at least one non-opaque pixel in the deep evidence set. Artifacts are represented by paths/hashes and are never embedded in protocol JSON.

## Known boundaries

Production rendering uses Driver Protocol v7 with Host-mediated tools, typed logical path arguments and session-bound detached tool jobs. `render.execute` uses protocol-owned cooperative request cancellation, observed progress and validated artifact reporting through `DriverExecutionContext`; `render.start/status/cancel/result` expose the same Host job substrate through stable semantic refs. Dynamic capabilities, child events and broker-native application refs remain disabled because this driver does not emit or require them. Registry packages do not yet provide an immutable, fully attested auxiliary runtime-bundle primitive; that remains the packaging gap for the Playwright/Firefox resource tree. External arbitrary Motion Canvas projects are not safely mutable and are therefore not accepted as managed semantic data.
