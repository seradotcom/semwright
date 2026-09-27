# Developer setup and verification discipline

Read [VERIFY.md](../VERIFY.md), [RELEASE_BLOCKERS.md](../RELEASE_BLOCKERS.md) and
[ACCEPTANCE.md](../ACCEPTANCE.md) before changing status labels. Existing architecture and
execution boundaries should be repaired and extended rather than replaced with parallel engines.

Use the Rust toolchain and components declared by the repository, including rustfmt and Clippy.
Keep dependency resolution reproducible with the committed lockfile and use `--locked` in
verification workflows. Do not invent dependency versions or infer lockfile contents from external
documentation.

A typical verification sequence is: source validation → formatting → compile checks → unit/property
tests → fake broker integration → CLI/MCP/cancellation → Clippy/docs → security checks → isolated
live backends. Keep failing tests and fix their causes. A successful probe for one adapter does not
substitute for the corresponding Rust or integration gate.

## Source checks

```sh
python scripts/verify-source.py
python -m unittest discover -s tests/python -v
node --test tests/js/bridge.test.mjs
gcc -Wall -Wextra -Werror -O2 tests/native/openat2.c -o /tmp/semwright-openat2-check
fixture=$(mktemp -d)
/tmp/semwright-openat2-check "$fixture"
python tests/python/cdp_live.py
```

`scripts/verify-local.py` is a bounded convenience runner for temporary-path cleanup and
machine-readable per-gate status. It records absent tools as `BLOCKED`, never `PASS`. Python
requirements for these checks are jsonschema, PyYAML and websocket-client. The fake-bpy suite does
not import a real Blender runtime. The native filesystem harness does not interact with a desktop.
The Chromium test launches an isolated sandboxed browser profile and removes it after completion.

## Rust tests and performance

Unit/property tests live next to pure types, policy, recipes, registry, protocol and backend
modules. `crates/core/tests/broker_contract.rs` exercises the fake broker path. Fuzz targets and
seeds live in `fuzz/`; use bounded smoke budgets from its README rather than unattended fuzz
campaigns. Benchmarks under `crates/core/benches` cover selectors, policy, encoding, registry,
compact snapshot construction and a fake end-to-end action.

Coverage claims must measure the intended workspace scope and name any deliberate exclusions.
Core and pure crates target the coverage threshold documented by project acceptance criteria.
Do not exclude difficult live adapters merely to improve an aggregate badge. Use cargo-llvm-cov
with the repository toolchain and retain scope metadata with published coverage evidence.

Source checks do not replace Rust compilation, JS/Python linting, shell validation, Nix evaluation
or GitHub Actions. Release evidence is authoritative only when the corresponding workflow,
environment and exact revision are recorded.
