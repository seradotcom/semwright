# Semwright Figma Driver

First-party semantic Figma integration built on the official Figma Plugin API plus an optional official REST transport.

```text
agent
  -> Semwright broker / policy / audit
  -> sandboxed semwright-figma-driver
  -> typed semantic capability
      -> authenticated 127.0.0.1 plugin bridge -> official Plugin API
      -> protected credential socket -> official Figma REST API
```

The production route does **not** patch `app.asar`, expose CDP, provide coordinate control as the primary architecture, or execute arbitrary JavaScript.

## Status

The semantic-completeness branch currently advertises 383 `driver.figma.*` capabilities: 324 typed Plugin API operations, 56 cloud operations (54 pinned official REST operations, one documented semantic discovery helper, plus `cloud.status`), and three local driver/session operations.

The Plugin API coverage compiler is pinned to `@figma/plugin-typings 1.139.0`. Its generated inventory covers 18 global interfaces, 14 auxiliary interfaces, 34 scene-node types, 213 global members, 49 auxiliary method entries, and 3,699 scene-node members with zero unclassified public method names.

Coverage means every pinned public surface is classified and has a semantic route or explicit policy classification. It does **not** substitute for protected acceptance against a real Figma account/file.

## Surface

The driver includes document/page/selection semantics; bounded node/property inspection and mutation; declarative compose/batch operations; Auto Layout; rich text and variable-font handling; vector networks; images/media/embeds; components, variants, instances and Slots; Variables including modern scopes/types; styles, libraries and shaders; snapshots/diffs; lint/a11y/design-system validation; prototyping; Motion Beta; viewport/editor state; FigJam tables/timer/diagram primitives; Slides; Buzz; Dev Mode/codegen/text-review/collaboration surfaces; artifact-backed static and animated exports; CSS/Tailwind/JSX/Storybook design-system exports; and official REST/cloud operations.

The REST transport never accepts credentials in capability arguments. It reads a bounded credential frame from a same-UID protected Unix socket and disables redirects. Without that owner-provisioned helper, `cloud.status` reports unconfigured and cloud operations fail closed.

## Verification

```sh
python3 crates/driver-figma/tools/catalog_consistency.py
node crates/driver-figma/tools/api_coverage.mjs
python3 crates/driver-figma/tools/rest_api_coverage.py
cargo test -p semwright-driver-figma --all-targets --all-features
cargo build -p semwright-driver-figma --features test-tools --bins
BIN_DIR=target/debug python3 crates/driver-figma/tools/e2e_driver.py
cd crates/driver-figma/plugin
npm ci && npm test && npm run typecheck && npm run build
```

Hosted native integration additionally runs the DriverProvider through the real Linux Driver Host sandbox. Heavy Rust, fuzz, coverage and conformance gates are expected to run in GitHub Actions.

## Security and acceptance boundary

The plugin bridge listens on loopback only and uses an ephemeral 256-bit pairing secret with server nonce/HMAC challenge-response. Requests are allowlisted and schema-bounded; sessions carry generations and observed document revisions so stale collaborative mutations fail instead of overwriting silently.

The REST transport uses official endpoints only, explicit operation metadata/scopes, bounded bodies, no redirects, secret-header marking and uncertain outcomes for ambiguous network failures on mutations. Plugin exports are held behind bounded artifact tokens/chunk reads and are also promoted as Driver Protocol v2 `JobArtifact` metadata so Jobs/Inspector can surface produced artifacts without embedding binary payloads in result JSON.

The repository's automated fake-host and sandbox results are not presented as real-Figma certification. Real Figma Design, FigJam, Slides, Buzz, Motion and collaboration acceptance still require an authorized disposable account/file.

See [API coverage](docs/API_COVERAGE.json), [REST coverage](docs/REST_API_COVERAGE.json), [security](docs/SECURITY.md), [capabilities](docs/CAPABILITIES.md), and [compatibility](docs/COMPATIBILITY.md).
