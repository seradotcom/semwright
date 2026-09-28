# Semwright

**A cross-platform semantic capability broker that turns applications and desktops into typed commands—not a stream of guessed clicks.**

> **Development snapshot, 0.9.0-dev.1. Not a verified release candidate.**
> The repository includes a committed `Cargo.lock`, Rust 1.98.1 as the development pin,
> and Rust 1.88 as its declared minimum supported Rust version. Hosted workflows exercise
> the core, native application integrations, platform hosts, packaging and supply chain.
> A workflow's existence or an older successful run is not evidence for a new commit.
> The pre-R16 observation of `241000c268d1bf1dc29d4e91a913097ac0d020cb` found 11 successful
> workflows and a failing Windows ARM64 native fixture. Independent security review remains
> **R16 OPEN**; physical Hyprland/mixed-display evidence remains limited under R06.
> Read [VERIFY.md](VERIFY.md), [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md), and the
> [pre-R16 state map](verification/pre-r16/PRE_R16_STATE_MAP.md) before granting desktop access.

```text
Agent intent                 Semwright authority                 Linux / application
"find the Export button"  →  schema → policy → exact selector  →  AT-SPI
"create a cube"           →  schema → policy → typed operation →  Blender API
"focus this window"       →  schema → policy → live reference →  compositor IPC
"do it again safely"      →  validated recipe → same broker   →  same narrow commands
```

The project contains source implementations of a Rust daemon, CLI, MCP frontend,
terminal inspector, command registry, Provider Runtime, MCP federation client, persistent
App Driver SDK/host, reference store, policy engine, metadata audit, recipe runner,
sandboxed plugin host, desktop backends, and application adapters. The runtime is now split
behind explicit platform contracts with Linux, macOS and Windows host implementations.
Linux has hosted native application/compositor evidence. Native macOS and Windows hosted
checks are distinct from interactive, consented desktop certification; their isolation
models and tested feature coverage are not interchangeable.
No model, cloud account, default shell, remote desktop service, arbitrary Python/JS
command, telemetry client, or root daemon is part of the product.

## What the interface looks like

These are intended CLI examples, **not a captured successful Rust run**:

```sh
semwright doctor
semwright --json capabilities list
semwright --json ui find --app org.gnome.TextEditor --role button --name Save
semwright commands describe ui.invoke
semwright ui invoke 'ui:<reference returned by this session>' --action click
semwright recipe run recipes/fake-export.yaml

# Owner-only MCP definition management; this does NOT grant broker policy authority:
semwright mcp upstream list

# Driver authoring/verification remains local owner tooling:
semwright --json driver validate ./driver.json
semwright --json driver conformance ./driver.json

# Static/local distribution is also owner-only and never grants driver policy authority:
semwright --json driver index validate ./registry/index.json
semwright --json --dry-run driver install ./registry/index.json libreoffice \
  --application-version 24.2
```

References are opaque, session-scoped, short-lived values. Do not paste the illustrative
reference above literally. A discovery result with two matching buttons remains two
candidates. The broker does not choose one and click it. Invocation requires a current
explicit reference and an action the target advertises.

## Agent Skills

Semwright interoperates with the open Agent Skills package shape without turning Skill prose into execution authority. Skills teach an agent when and how to combine Semwright capabilities; the Broker remains the single schema/policy/provenance/audit boundary.

```sh
semwright skill validate ./skills/semwright-core
semwright skill inspect ./skills/semwright-core
semwright skill doctor ./skills/semwright-cross-app-artifacts
semwright skill test ./skills/semwright-workflow-distillation
semwright skill lock ./my-skill
semwright skill bundle ./my-skill ./my-skill.zip
```

A standard Skill without `.semwright/` metadata remains valid. Optional requirements/locks support deterministic compatibility and descriptor-drift checks but never grant permissions. Semwright never auto-executes a Skill's `scripts/`. Repeated successful Skill-guided execution can instead be distilled through the existing Workflow Distillation path into a verified `recipe.<slug>.run` capability. See [Agent Skills](docs/skills.md).

## Why not screenshot-first?

Semantic interfaces expose identity, roles, names, actions and state. Semwright starts
there, or with a richer application API. Input fallback is a distinct capability and
must prove the intended window is focused. No failed mutation automatically falls back
to another backend. Interactive screenshot capture is separate; there is no vision model.

The distinguishing design is the **shared authorization boundary**: CLI, MCP, inspector,
recipes, providers, and plugin commands cannot obtain a more privileged execution path
by choosing a different frontend. Passing compiler and CI gates is not a substitute for
the independent security review and live-system evidence still listed as blockers.

## Build and first validation

For the verified Linux path, use a disposable Linux account or VM first. Do not use `sudo` to run the daemon.
Use the repository-pinned Rust toolchain and the committed lockfile. Network access may
still be required to populate an empty Cargo cache.

```sh
# From this source tree:
./scripts/dev/bootstrap.sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
./scripts/dev/fake-smoke.sh
```

Do not mark a failed gate as optional or delete a test to get a green result.
The full gate script also requires `cargo-audit` and `cargo-deny`:

```sh
./scripts/ci/rust-gates.sh
```

Source-only checks that do not compile Rust can be repeated with:

```sh
python -m pip install jsonschema PyYAML websocket-client
python scripts/verify-local.py --with-chromium
```

That command intentionally returns nonzero when required tools are absent, even when
all available component checks pass. See [development](docs/development.md).

## Fake-desktop path, after a successful build

The [fake smoke script](scripts/dev/fake-smoke.sh) creates a private temporary runtime,
starts **only** the fake backend, runs discovery and a complete export recipe, prints
metadata audit, stops its own broker, and removes its own temporary directory. It does
not touch the live desktop or leave an unattended process running.

For a live observe-only run after that succeeds:

```sh
mkdir -p "$HOME/.config/semwright"
chmod 700 "$HOME/.config/semwright"
install -m 600 config/observe.toml "$HOME/.config/semwright/daemon.toml"
target/debug/semwrightd --config "$HOME/.config/semwright/daemon.toml"
# A second terminal, in the same graphical login session:
target/debug/semwright --json doctor
target/debug/semwright ui snapshot --max-nodes 100
```

Do not overwrite an existing configuration using this example. A foreground daemon
with `--approval-console` is required for actions classified as sensitive. The operator
responds on the daemon's own terminal—not through an agent-accessible confirmation tool.

## Components and evidence

| Component | Delivered | Evidence boundary |
|---|---|---|
| Core, CLI, MCP, inspector, policy and refs | Typed broker and common authority path | Linux x86_64/ARM64 unit/property/integration gates; exact-SHA outcomes are recorded separately |
| Platform contracts | Portable API, shared services and Linux/macOS/Windows hosts | Cross-compilation is not native acceptance; native hosted runs are not interactive certification |
| macOS | AX/CoreGraphics/ScreenCaptureKit/NSPasteboard and native host services | Intel/Apple Silicon hosted build and noninteractive checks; authorized TCC/live acceptance remains separate |
| Windows | UIA, input/capture/IPC and restricted process-launch implementation | Native x64/ARM64 and compatibility jobs exist; the preflight snapshot has an ARM64 UIA failure, and interactive certification is not claimed |
| AT-SPI, X11, Sway, GNOME/KWin, Hyprland | Native semantic and compositor backends, revisioned refs and recovery | Hosted GTK/Qt, Xvfb/Openbox, headless Sway and Plasma jobs; historical real-login/nested evidence has explicit environment/SHA limits |
| Portal, EIS and PipeWire | Consented sessions, restore tokens, clipboard, input sender and bounded frame capture | Private D-Bus/EIS fixtures and a real synthetic PipeWire stream; historical isolated GNOME/Plasma VM keyboard evidence is not physical multi-display certification |
| Filesystem and artifact handoff | Explicit source/destination grants, platform confinement and bounded binary transfer | Linux openat2 and grant/digest regressions; no claim that every OS implements the same confinement mechanism |
| Plugins and Driver Host | Pinned identities/descriptors, resource limits and platform sandbox admission | Executed Linux hostile fixtures; Windows has separate native authority tests and limitations; neither is independent security review |
| MCP federation | Sandboxed stdio upstreams and owner-only registry | Executed real protocol fixtures, central policy, cancellation and catalog/crash tests; launch fails closed without the required sandbox |
| Driver distribution | Non-executing packages, static indexes and bounded companion files | Integrity/compatibility and install/update/remove checks; package hashes do not establish remote publisher identity |
| Blender | Sandboxed deep DriverProvider plus legacy main-thread add-on | Real Blender hosted smoke, typed operations/RNA inspection and render/save; active owner-runtime/export changes are not part of the observed baseline |
| Chromium | Private-profile CDP adapter and semantic browser surface | Real browser navigation, multi-frame/ref/download limits and cleanup fixtures; origin restrictions are not a firewall |
| LibreOffice, MLT, KiCad and OBS | Application-specific DriverProviders | Separate curated/native/fake-protocol matrices; consult each driver's verification record rather than inferring complete native-API coverage |
| Figma | Authenticated loopback bridge to the official Plugin API | Typed/plugin/fake-host and sandboxed protocol evidence; disposable real-Figma acceptance remains a separate requirement |
| Godot and Motion Canvas | Typed application/project providers | Driver-specific conformance and real-runtime workflows; no generic arbitrary-code execution or blanket application certification claim |
| Events, jobs and Workflow Distillation | Session-scoped jobs/events, progress/artifacts, MCP Tasks and gated learned recipes | Broker re-entry, privacy, cancellation, drift and promotion tests; recipes are not transactions and do not grant permissions |
| Packaging and supply chain | Reproducible native tar/deb, Nix, SBOMs and scoped attestations | Executed hosted jobs; release admission remains blocked by independent/live evidence requirements |

Full details: [compatibility](docs/compatibility.md), [manual tests](docs/manual-testing.md),
[acceptance resolution](ACCEPTANCE.md), [verification](VERIFY.md).

## MCP

The frontend uses the official `rmcp` Rust SDK. It deliberately presents a small
discovery/gateway surface instead of exposing every internal capability as a static MCP
tool. Tool discovery returns the same registry schemas used by the broker. A generic
local MCP client configuration after installing:

```json
{
  "mcpServers": {
    "semwright": {
      "command": "/home/YOUR_USER/.local/bin/semwright-mcp"
    }
  }
}
```

Start the broker separately in the same user session. The configuration above does not
start it, authorize mutations, or approve portal dialogs. See [MCP](docs/mcp.md).

Semwright also has a governed [MCP federation](docs/mcp-federation.md) provider.
Owner-configured stdio servers are imported into the same capability registry and remain
subject to normal broker policy, operator approval, provenance and audit. Operators manage
definitions locally with `semwright mcp upstream ...`; those local commands never add a
policy grant, so registering a server is distinct from authorizing its tools. The initial
launcher stages digest-verified bytes, scrubs inherited environment and requires the platform
sandbox. Network and filesystem authority are separate owner grants; unavailable isolation
fails closed. This does not protect the broker from an unrelated hostile process already
running outside the sandbox with the same UID, nor make the OS sandbox a formal kernel proof.

## Install, extend, inspect

[Installation](docs/installation.md) covers local binaries, the optional user service,
checksums, uninstall, Debian packaging and the Nix expression. No installer silently
uses `sudo`, enables a plugin, requests portal consent, or downloads an opaque binary.

[Recipes](docs/recipes.md) replace repeated improvisation with typed bindings and explicit
assertions. [Plugins](docs/plugins.md) add narrow one-shot sandboxed commands. The
[App Driver SDK](docs/drivers.md) adds persistent application providers with owner-assigned
identity, digest-pinned capabilities and executable conformance. [Driver distribution](docs/driver-distribution.md)
adds non-executing local packages and static indexes without granting policy authority.
[Events and jobs](docs/events-jobs.md) document source-bound event delivery and bounded long-operation
lifecycle. [Semantic video domain](docs/video-domain.md) defines the backend-neutral timeline,
edit and conformance core reused by concrete video drivers. [The inspector](docs/inspector.md)
is read-only and uses the same broker socket.
Application instructions: [Blender](adapters/blender/README.md),
[Chromium](adapters/chromium/README.md), [LibreOffice](crates/driver-libreoffice/README.md),
[MLT video](crates/driver-mlt-video/README.md), [Figma](crates/driver-figma/README.md),
[Godot](crates/driver-godot/README.md), and [KiCad](integrations/kicad-driver/README.md).
Desktop bridges: [GNOME](bridges/gnome/README.md),
[KWin](bridges/kwin/README.md).

## Security boundary

Observe is the default. Clipboard content, screenshots, raw input, application launching,
plugins and app-native mutation each require explicit permissions. Destructive, secret,
code-execution and privilege-sensitive requests additionally require an external operator.

A Unix UID is **not** a sandbox against another malicious process with that same UID.
An agent separately given an unrestricted shell can bypass this product's mediated
command surface. A browser origin list is not a network firewall. Existing application
processes such as Blender are not sandboxed by the broker. See [SECURITY.md](SECURITY.md)
and [permissions](docs/permissions.md) before granting access.

## Project status and licensing

Semwright is a working name; namespace/trademark clearance and publication are unfinished.
This archive does not represent an existing public GitHub release or a promised popularity
outcome. Original core source is dual-licensed **MIT OR Apache-2.0**. The isolated
`integrations/kicad-driver` subtree is **GPL-3.0-or-later** and carries its own notices; it is
not relicensed as core source. `Cargo.lock` and `deny.toml` anchor the dependency/license checks;
their successful execution does not close R16.
[Contributing](CONTRIBUTING.md), [governance](GOVERNANCE.md),
[changelog](CHANGELOG.md), and [the original requirements](docs/requirements/START_HERE.md)
make the requested scope and unfinished work explicit.
