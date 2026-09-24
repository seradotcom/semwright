# Motion Canvas security model

The managed format exists because arbitrary TypeScript is executable code. The first-party driver deliberately does **not** expose eval, arbitrary TS/TSX, shell commands, arbitrary npm install, arbitrary remote scripts or arbitrary easing/generator expressions.

## Trust boundaries

Agent input is untrusted semantic data. Driver Protocol descriptors are strict, digest-pinned schemas. The Rust driver owns policy-relevant validation and never delegates authorization to Node or the browser.

The Driver Host is the execution boundary. Production rendering requires Bubblewrap + Landlock, named owner grants, a pinned driver ELF, `network=false` and bounded process/file/CPU/address-space resources. There is no unsandboxed fallback. Motion Canvas requests 16 GiB of virtual address space while retaining bounded CPU/process/file limits; the SDK default remains 512 MiB. The larger `RLIMIT_AS` ceiling permits Chromium's sparse virtual mappings and does not pre-allocate or grant 16 GiB of resident RAM.

Ubuntu 24.04 additionally restricts unprivileged user namespaces through AppArmor. CI loads the distro `bwrap-userns-restrict` profile specifically for `/usr/bin/bwrap`; it does not disable `kernel.apparmor_restrict_unprivileged_userns` system-wide. This lets Bubblewrap create the isolated namespaces it needs while preserving Ubuntu's global user-namespace mitigation for unrelated processes.

The Motion runtime is an explicit read-only owner grant. Fontconfig is a separate read-only grant mapped only to `/etc/fonts`, because Driver Host otherwise constructs a minimal `/etc`. Node, renderer helper and the full Chromium executable are each verified against SHA-256 before use. Runtime configuration parsing is strict and bounded; malformed or stale tools make rendering unavailable.

## Files and assets

The project store rejects noncanonical roots, traversal, absolute/remote paths and symlink escapes. Semantic/project/media reads are bounded. Assets record SHA-256 and byte length; copies are rechecked before generated-project use.

SVG is parsed as a restricted local structural subset: scripts, event handlers and external references are rejected. Image dimensions/decoded allocation are bounded. Code-node text, prompt-injection-looking strings, template syntax and backticks remain display data and are escaped by deterministic codegen.

Generated source is written to a driver-owned content-addressed tree; agent text is never concatenated as executable TypeScript syntax. Dependency versions are exact and the lockfile is part of deterministic output. Runtime package scripts are disabled in CI installation.

## Browser

The browser uses a disposable profile/context, no user profile, no credentials or extensions. Built assets are fulfilled through request interception from the synthetic `semwright.invalid` origin and other page requests are aborted. No Vite server listens on loopback or LAN. The only listener is Chromium's ephemeral CDP endpoint on `127.0.0.1` inside Bubblewrap's isolated `network=false` namespace, which has loopback only and is not reachable from the host network.

Full Chromium runs in new-headless mode inside the mandatory Driver Host Bubblewrap + Landlock boundary; the helper exposes no agent-controlled browser flags. After verifying the Driver Host marker it spawns the pinned executable directly with a fixed `--no-sandbox` argument because Driver Host is the outer sandbox, then attaches Playwright over a kernel-selected loopback CDP port. The prior Playwright `launch()` path repeatedly terminated Chromium with `SIGTRAP` in CI. The replacement does not widen authority: filesystem grants remain explicit, `network=false` leaves only private loopback, external page requests are aborted, and direct helper execution fails closed. The Rust parent pins `TMPDIR` and XDG state to the job-specific owner-granted output directory and owns the whole process group for cancellation.

## Jobs and artifacts

Render jobs allow at most two active jobs and a bounded retained registry. Timeout/cancel terminates the process group and removes partial output. Child stdout/stderr are bounded.

PNG artifacts must have exact expected sequential names/count, bounded decode size and planned dimensions. Transparent renders must prove at least one non-opaque pixel. Artifacts are represented by paths/hashes and are never embedded in protocol JSON.

## Known boundaries

The integrated SDK supports Driver Protocol v2, but this driver deliberately negotiates v1 in this PR and does not claim v2 child events/progress/cancellation; render cancellation is driver-local. Registry packages do not yet provide a generic multi-tool runtime distribution primitive. External arbitrary Motion Canvas projects are not safely mutable and are therefore not accepted as managed semantic data.
