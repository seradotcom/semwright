# Godot completeness boundary

This document defines what “complete” means for the first-party Semwright Godot driver.

Completeness is measured against a curated semantic-computer-I/O boundary, not against a
one-to-one mirror of every Godot ClassDB method. Arbitrary `Object.call`, arbitrary GDScript
execution, shell access and unrestricted server APIs are deliberately outside the boundary.

The certified runtime target for this closeout is **Godot 4.7.2-stable on Linux x86_64**.
macOS, Windows and Linux ARM64 require their own real-editor acceptance before they can inherit
that certification.

## 3. Semantic-domain completeness — complete

The curated authoring layer has typed inspection and/or mutation semantics for all domains in
`SEMANTIC_DOMAINS.md`: projects, scenes, nodes, resources, project settings, autoloads,
InputMap, assets/imports, export presets, localization, AnimationPlayer, recursive
AnimationTree graphs, TileMap/TileSet, GridMap/MeshLibrary, paths/curves, navigation, physics,
audio, particles, rendering, UI/themes, skeleton/rigging, multiplayer scene metadata, editor
state, managed GDScript, signals/groups, validation, runtime tests and exports.

A domain is not counted as covered merely because generic `node.patch` can write one of its
properties. Covered domains expose schemas and operations corresponding to the meaning of that
domain.

## 2. Generic semantic substrate — complete

The generic layer provides bounded semantic access for useful engine/project objects that do not
need a dedicated domain capability:

- ClassDB and project-class read-only introspection;
- API search/describe without dynamic method invocation;
- broad typed Variant encoding for structured Godot values;
- generic node/resource create, inspect and patch;
- semantic snapshots and bounded diffs;
- provider-owned node/resource/scene refs with generation/revision/fingerprint freshness;
- broker-native opaque ref materialization and provider validation.

This layer deliberately does not convert discovered methods into executable tools.

## 1. Runtime and infrastructure — complete for the certified Linux target

The production path is:

```text
Agent
  -> Broker / Policy / Audit / RefStore
  -> Driver Host
  -> Driver Protocol v3
  -> sandboxed Godot Rust driver
  -> Host-managed loopback proxy
  -> private Unix socket
  -> authenticated Godot EditorPlugin
  -> official Godot editor / scene / resource APIs
```

The certified path includes:

- Driver Protocol v3 with events, progress, artifacts, health, cooperative cancellation and
  native-reference validation;
- strict capability input/output schemas and descriptor digest pinning;
- bubblewrap + Landlock confinement with no unsandboxed fallback;
- loopback-only bridge authority without granting ambient driver network access;
- owner pairing material delivered through first-class read-only secret mounts;
- shared Broker RefStore integration for provider-owned Godot refs;
- lifetime resource limits plus bounded Linux/Windows per-operation CPU accounting for persistent drivers;
- digest-pinned secondary tools staged by Driver Host as sealed executable payloads;
- Driver Package v2 companion artifacts with explicit paths, digests, size budgets and no
  automatic Godot-project activation;
- real Godot 4.7.2 editor acceptance, save/reload, runtime, export, cancellation and artifact
  verification.

## Deliberate exclusions

The following do not prevent completeness under this boundary:

- mirroring every Godot ClassDB method as a Semwright capability;
- arbitrary `Object.call` or arbitrary GDScript evaluation;
- C#/.NET execution and arbitrary GDExtension/native-code loading;
- arbitrary third-party EditorPlugin execution;
- unrestricted NavigationServer/RenderingServer/PhysicsServer calls;
- XR/device control without a separate provider/security design;
- automatic installation or activation of the companion plugin inside user projects.

Godot projects remain executable software. A typed semantic driver does not turn an untrusted
project into passive data.

## Cross-platform status is separate from completeness

The semantic model and driver source are portable, but real-editor certification is
platform-specific. Linux x86_64 is the certified target for this closeout. A successful compile
on macOS, Windows or Linux ARM64 is not substituted for real Godot acceptance and platform-host
confinement evidence.

## Closeout criterion

For the certified Linux target, there are no unresolved P0/P1 Godot-specific Driver SDK blockers.
Future work in this area is expansion or platform certification, not completion of the core
Godot semantic runtime.
