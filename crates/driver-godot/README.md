# Semwright Godot Driver

First-party semantic Godot 4.x integration built on an authenticated local EditorPlugin bridge plus a pinned Godot runner for validation, runtime tests and exports.

```text
agent
  -> Semwright broker / policy / audit
  -> sandboxed semwright-godot-driver
  -> Host-managed loopback proxy / private Unix socket
  -> authenticated WebSocket bridge
  -> Semwright Godot EditorPlugin
  -> official Godot editor / scene / resource APIs
```

The driver does not expose arbitrary GDScript evaluation, OS.execute, shell execution, coordinate automation or unrestricted object-method invocation.

## Status

The current catalog exposes **188 typed `driver.godot.*` capabilities** with operation-specific strict input/output schemas. Driver Protocol v3 negotiates cooperative cancellation, child events, progress, artifacts, health and broker-native reference validation. The production Rust driver has been exercised end-to-end against both an independent fake editor and Godot 4.7.2-stable.
The real acceptance harness creates a disposable Lab Room through the production driver with both 3D and 2D authoring subtrees. It writes resources and managed scripts, persists keyboard/mouse/gamepad InputMap actions, exercises typed 2D/3D navigation and physics, creates signals and animation keyframes, saves/reloads the scene, validates and runs it headlessly, exports a PCK artifact and verifies cancellation. It also verifies real Godot editor events crossing the child-event interface.

## Surface

The curated surface covers project/session inspection, scenes and nodes, project window/rendering/physics/layer settings, autoloads, InputMap, resources, managed GDScript, signals, assets/imports, export presets, localization, animations and recursive AnimationTree state-machine/BlendTree/BlendSpace graphs, TileMap/TileSet, GridMap/MeshLibrary, paths/curves, navigation, physics, audio, particles, rendering, UI/themes, skeletons, multiplayer scene-replication authoring, editor selection/state, bounded editor runs, semantic snapshot diff, validation, runtime tests, pack/build export and deterministic movie capture. A bounded generic substrate additionally exposes read-only ClassDB/project-class introspection, a broad typed Variant codec and provider-owned node/resource/scene refs with freshness validation; discovered methods are never dynamically invoked and refs are never executable handles.

`export.build` requires owner-installed Godot export templates. `movie.capture` requires an owner-configured X11 display; the driver deliberately refuses that path without one because Godot 4.7.2's dummy headless renderer can crash under `--write-movie`.

## Build and verification

```sh
cargo fmt --all -- --check
cargo test -p semwright-driver-godot --all-targets
cargo clippy -p semwright-driver-godot --all-targets --all-features -- -D warnings
cargo build -p semwright-driver-godot --bin semwright-godot-driver
```

Hosted native integration additionally downloads the pinned official Godot 4.7.2 Linux build, checks its SHA-256, runs authenticated real-editor acceptance and runs the driver through the real Linux Driver Host sandbox.
## Pairing and project safety

Owner configuration assigns each project a 256-bit project identifier and a secret-file reference. In production the pairing material is delivered read-only under `/run/secrets`; inline secrets are accepted only in explicit development mode. The plugin sends a random nonce, the bridge returns a server challenge, and both sides authenticate the same bounded transcript with HMAC-SHA256 before a session is usable. Under Driver Host the driver listens on a private Unix socket and the Host exposes only the configured 127.0.0.1 port; direct acceptance mode retains a loopback TCP listener.

Opening/importing arbitrary Godot projects is a code-execution boundary: projects can contain `@tool` scripts, EditorPlugins, GDExtensions, custom importers and other executable content. Configure only owner-approved project roots and use disposable fixtures for untrusted projects.

Managed script writing rejects `@tool` and is path-confined to the configured project, but scripts remain project source code and execute when the owner later runs the project. Policy must continue to classify those capabilities accordingly.

## Installation

Driver Package v2 can carry the Rust driver together with the reviewed EditorPlugin companion tree:

```sh
semwright driver package create driver.manifest.json semwright-godot.swdp \
  --companion-list crates/driver-godot/companions.list
```

The companion list is checked against `integrations/godot/addons/semwright/` in tests. Package installation stores these files privately under the installed driver version's `companions/` subtree; it does **not** copy them into a Godot project or enable the plugin. Activation remains an explicit owner action: copy the reviewed `addons/semwright/` subtree into the target project's `res://addons/semwright/` and enable it in Godot.

Start from `driver.manifest.example.json`, pin the driver and Godot runtime digests, provide owner grants for config/project/output, the pairing secret file and the Godot runtime tool. Driver Host exposes the editor bridge through loopback-only authority without granting the driver ambient network access, delivers pairing material through a read-only secret mount, and stages the Godot runtime as a sealed secondary executable.

See [security](docs/SECURITY.md), [compatibility](docs/COMPATIBILITY.md), [capabilities](docs/CAPABILITIES.md), [semantic-domain completeness](docs/SEMANTIC_DOMAINS.md), and [SDK gaps](docs/SDK_GAPS.md).
