# Semwright

**A local capability broker that turns Linux applications and desktops into typed commands—not a stream of guessed clicks.**

> **Development snapshot, 0.9.0-dev.1. Not a verified release candidate.**
> The accepted development line has a committed `Cargo.lock`, pins Rust 1.98.1, and has
> passed hosted x86_64/ARM64 format, check, build, Clippy, workspace tests, doctests,
> rustdoc, fake end-to-end, dependency, coverage, bounded-fuzz and real Rust Chromium gates.
> Provider Runtime, governed stdio MCP federation, the persistent App Driver SDK with
> sandboxed conformance tooling, and non-executing static/local driver distribution are merged
> after exact-head green CI. A real LibreOffice/UNO
> deep driver now exercises that SDK through the normal CLI/broker/policy path, while Chromium
> has a hosted real-browser integration with bounded handling of transient target metadata. Live
> desktops, real Blender, broader driver coverage, packaging and independent security review
> remain incomplete. Read
> [VERIFY.md](VERIFY.md) and [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md) before granting
> desktop access.

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
sandboxed plugin host, desktop backends, and application adapters.
No model, cloud account, default shell, remote desktop service, arbitrary Python/JS
command, telemetry client, or root daemon is part of the product.

## What the interface looks like

These are intended CLI examples, **not a captured successful Rust run**:

```sh
computerctl doctor
computerctl --json capabilities list
computerctl --json ui find --app org.gnome.TextEditor --role button --name Save
computerctl commands describe ui.invoke
computerctl ui invoke 'ui:<reference returned by this session>' --action click
computerctl recipe run recipes/fake-export.yaml

# Owner-only MCP definition management; this does NOT grant broker policy authority:
computerctl mcp upstream list

# Driver authoring/verification remains local owner tooling:
computerctl --json driver validate ./driver.json
computerctl --json driver conformance ./driver.json

# Static/local distribution is also owner-only and never grants driver policy authority:
computerctl --json driver index validate ./registry/index.json
computerctl --json --dry-run driver install ./registry/index.json libreoffice \
  --application-version 24.2
```

References are opaque, session-scoped, short-lived values. Do not paste the illustrative
reference above literally. A discovery result with two matching buttons remains two
candidates. The broker does not choose one and click it. Invocation requires a current
explicit reference and an action the target advertises.

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

Use a disposable Linux account or VM first. Do not use `sudo` to run the daemon.
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

Local checks that do not compile Rust can be repeated with:

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
target/debug/computerctl --json doctor
target/debug/computerctl ui snapshot --max-nodes 100
```

Do not overwrite an existing configuration using this example. A foreground daemon
with `--approval-console` is required for actions classified as sensitive. The operator
responds on the daemon's own terminal—not through an agent-accessible confirmation tool.

## Components and evidence

| Component | Delivered | Evidence in this handoff |
|---|---|---|
| Rust core, broker, CLI, MCP, inspector | Source + Rust unit/property/integration tests | Hosted development line compiles and executes on x86_64 + ARM64 under the exact-SHA quality matrix |
| AT-SPI, Sway, Hyprland, X11, GNOME/KWin clients | Native backend source + Rust tests | Compiled/tested in hosted baseline; no live compositor matrix |
| GNOME/KWin bridges | JavaScript source + shared-contract tests | Node contract tests; not a live shell/runtime test |
| RemoteDesktop portal, interactive screenshot | Native D-Bus source + Rust lifecycle tests | Compiled/tested; no accepted live portal/EIS session |
| EIS/libei, PipeWire pixel stream, AT-SPI delta snapshots | Incomplete/deferred paths | Explicit release blockers; not relabelled as live-only evidence |
| Blender | Python add-on + Rust client | Mocked host coverage; no accepted real Blender/RNA/addon run |
| Chromium | Isolated-profile Rust CDP adapter | Real Rust hosted integration passes on this development line, including close/stale-ref invalidation and owned-profile cleanup |
| Scoped filesystem | Rust scoped implementation + native harness | Rust tests plus native openat2 checks in hosted baseline |
| Plugins | SDK, digest pinning, bubblewrap + Landlock source | Compiled/unit-tested; hostile sandbox conformance still open |
| MCP federation | Governed stdio provider + owner-only upstream registry | Merged after green x86_64/ARM64 CI with real fixture handshake/tool import, policy mediation, cancellation, dynamic refresh, crash invalidation and lifecycle smoke; same-UID upstream sandboxing remains open |
| App Driver SDK | Versioned persistent driver protocol + sandbox host + developer CLI | Merged after hosted driver-conformance: pinned fixture handshake/catalog/health/execute/shutdown, broker smoke and generated-driver compile |
| Driver distribution | Non-executing `.swdp` packages + static/local index | Hosted package/index tests and install/update/remove smoke; SHA-256 integrity/compatibility only, not publisher signatures or a marketplace |
| LibreOffice | Sandboxed persistent UNO DriverProvider | Real hosted Writer create/read, Calc create/get/set and PDF export through CLI -> daemon -> broker -> driver; curated seven-capability surface, not full UNO |
| MLT video | Sandboxed persistent semantic timeline DriverProvider | 68-capability bounded model with 206 Rust tests and host-sandbox conformance; real MLT/Kdenlive/Shotcut round-trip certification remains pending |
| KiCad | Separately licensed GPL IPC DriverProvider integration | Rust/Go build, native protocol tests and fake IPC host-sandbox conformance; real KiCad interoperability remains pending |
| Events/jobs | Provenance-aware event stream + bounded session-scoped jobs | Job execution re-enters normal policy/audit; cancellation, session privacy and revocation are integration-tested; generic progress/artifact/task mapping remains follow-on work |

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
definitions locally with `computerctl mcp upstream ...`; those local commands never add a
policy grant, so registering a server is distinct from authorizing its tools. The initial
launcher is digest-pinned and environment-scrubbed but is **not** a sandbox against a
malicious same-UID executable.

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
lifecycle. [The inspector](docs/inspector.md)
is read-only and uses the same broker socket.
Application instructions: [Blender](adapters/blender/README.md),
[Chromium](adapters/chromium/README.md), [LibreOffice](crates/driver-libreoffice/README.md),
[MLT video](crates/driver-mlt-video/README.md), and
[KiCad](integrations/kicad-driver/README.md). Desktop bridges: [GNOME](bridges/gnome/README.md),
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
not relicensed as core source. Dependency license resolution is not complete without a lockfile.
[Contributing](CONTRIBUTING.md), [governance](GOVERNANCE.md),
[changelog](CHANGELOG.md), and [the original requirements](docs/requirements/START_HERE.md)
make the requested scope and unfinished work explicit.
