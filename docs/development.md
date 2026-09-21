# Developer setup and verification discipline

Read [VERIFY.md](../VERIFY.md), [RELEASE_BLOCKERS.md](../RELEASE_BLOCKERS.md) and
[ACCEPTANCE.md](../ACCEPTANCE.md) before changing status labels. The existing source should
be repaired, not replaced with another architecture or a second execution engine.

An installed current stable Rust toolchain with rustfmt/clippy is needed. This handoff
uses a floating `stable` channel because no compiler ran; select and record a tested exact
version once dependency resolution succeeds. Generate Cargo.lock, review it, then use
`--locked`. Do not invent a lockfile or infer its versions from documentation pages.

Suggested sequence: generate/review lock → cargo fmt → check → unit tests → fake broker
integration → CLI/MCP/cancellation → Clippy/docs → security checks → live backends in a
throwaway desktop account. Keep source-level tests that fail and fix their causes. A
successful Python CDP probe does not close the Rust Chromium adapter gate.

## Available local checks

```sh
python scripts/verify-source.py
python -m unittest discover -s tests/python -v
node --test tests/js/bridge.test.mjs
gcc -Wall -Wextra -Werror -O2 tests/native/openat2.c -o /tmp/semwright-openat2-check
fixture=$(mktemp -d)
/tmp/semwright-openat2-check "$fixture"
python tests/python/cdp_live.py
```

Prefer `scripts/verify-local.py` for temporary-path cleanup and machine-readable per-gate
status. It uses bounded timeouts and records absent tools as BLOCKED, never PASS. Python
requirements for these checks are jsonschema, PyYAML and websocket-client. No real Blender
is imported by the fake-bpy unit suite. No live desktop is touched by the native filesystem
harness. The Chromium test launches its own isolated sandboxed browser and removes its
own profile; it does not connect to an existing browser.

## Rust tests and performance

Unit/property test source lives next to pure types/policy/recipes/registry/protocol and in
backend modules. `crates/core/tests/broker_contract.rs` exercises the fake broker path.
Fuzz source/seeds live in `fuzz/`; use bounded smoke budgets from its README, not unattended
unlimited fuzzers. Benchmark source under `crates/core/benches` covers selectors, policy,
encoding, registry, compact snapshot construction and a fake end-to-end action. None ran.

Coverage should measure the entire workspace before claiming aggregate quality. Core/pure
crates should aim for the brief's 90% line target; no coverage value has been obtained.
Do not exclude difficult live adapters to inflate a badge. Use cargo-llvm-cov with an exact
recorded toolchain and report both complete scope and any deliberate exclusions.

Source checks are not Rust parsing, full JS/Python lint, shellcheck, Nix evaluation or
GitHub Actions validation. The workflow files use readable version refs today; pin reviewed
immutable revisions before releases. No CI badge is advertised as green.
