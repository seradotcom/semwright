# Semwright Figma Driver

First-party semantic Figma integration built on the official Figma Plugin API.

```text
agent
  -> Semwright broker / policy / audit
  -> sandboxed semwright-figma-driver
  -> authenticated 127.0.0.1 WebSocket bridge
  -> Semwright Figma development plugin
  -> official Figma Plugin API
```

The production route does **not** patch `app.asar`, expose CDP, use coordinate automation, or provide arbitrary JavaScript evaluation.

## Status

The driver advertises 91 `driver.figma.*` capabilities: 88 bridge-backed operations plus three local driver/session operations. Inputs are operation-specific, bounded schemas and unsupported operations are not advertised.

Automated evidence covers the Rust driver, authenticated bridge, independent fake-Figma E2E, plugin runtime, catalog consistency and sandboxed DriverProvider conformance in hosted CI. Real Figma acceptance remains a separate protected/manual gate using a disposable file.

## Surface

The current curated surface includes documents/pages/selection, node inspection and mutation, Auto Layout, paints/strokes/effects, text/fonts, components/variants/instances, variables and design-system extraction, snapshots/diffs, prototyping reactions/flows, Figma Motion Beta, FigJam primitives and Dev Mode CSS inspection.

## Build

```sh
cargo build -p semwright-driver-figma --features test-tools --bins
cd crates/driver-figma/plugin
npm ci
npm test
npm run typecheck
npm run build
```

Load `plugin/manifest.json` as a Figma development plugin after building it. The plugin manifest is deliberately limited to Figma/FigJam, `documentAccess: dynamic-page`, and the fixed development loopback endpoint `ws://127.0.0.1:38471`.

The Driver Host manifest must be owner-configured. Start from `driver.manifest.example.json`, replace the executable path and SHA-256, and explicitly allow driver network access. The current Driver Manifest expresses network as a boolean grant; the driver itself binds only to loopback. This integration uses Semwright Driver Protocol v2 and negotiates child events, while cancellation, progress, artifacts and dynamic capabilities remain disabled until the Figma implementation can satisfy those contracts honestly.

## Pairing

The driver creates an ephemeral 256-bit secret for each process lifetime. Invoke `driver.figma.pairing.begin` through Semwright to obtain the loopback port and pairing code, then enter those values in the plugin UI. The server issues a fresh nonce and the plugin answers with HMAC-SHA256 before the session becomes usable.

Document sessions carry connection generations and observed revisions. Mutations can require `expected_revision`; stale collaborative state returns a conflict instead of overwriting silently.

## Verification

```sh
python3 crates/driver-figma/tools/catalog_consistency.py
cargo test -p semwright-driver-figma --all-features
cargo build -p semwright-driver-figma --features test-tools --bins
BIN_DIR=target/debug python3 crates/driver-figma/tools/e2e_driver.py
```

The native integration workflow additionally runs the driver through the real Linux Driver Host sandbox and pairs it to the independent fake Figma process.

No automated fake-host result is presented as real Figma acceptance. Certification against Figma Design, FigJam and Motion requires an authorized disposable account/file and remains documented as pending until executed.

## Security

The bridge listens on loopback only, uses challenge-response pairing, bounds requests and pending work, rejects replayed/stale generations, and exposes a typed allowlisted dispatcher rather than `eval`. Figma document content is untrusted data. See [SECURITY](docs/SECURITY.md), [bridge protocol](docs/BRIDGE_PROTOCOL.md), [capabilities](docs/CAPABILITIES.md), and [SDK gaps](docs/SDK_GAPS.md).
