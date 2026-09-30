# Release impact — F

No release, tag, main merge, protection change or R16 closure is authorized by this role.

Added package: `semwright-effect-conformance`, inheriting workspace version/license/toolchain. Runtime dependency direction is F -> A semantic-composition; project-graph is dev-only. No duplicate EffectClass, Finding, VerificationReport, clock, vault or policy engine was introduced.
A C0 `26602e4...` and C P0 `6ee52b4...` are pinned consumed dependencies. Shared A wire/canonical formats remain unchanged. F schemas are additive and versioned.

Implementation source `0afad4b4ceac59434ae938d246f96d8d4351498e` has:
- F release 36683875483 PASS on Linux/Windows/macOS; Linux native Godot/Blender and mutations PASS.
- Quality 36683883701 PASS including source hygiene, fmt/check/build/clippy/tests/docs and static lints.
- Dependency/coverage/fuzz 36683883572 PASS including cargo-audit and cargo-deny.
- Packaging 36683883626 PASS including deterministic package and user install lifecycle on x86_64/aarch64 Linux.
- Supply-chain 36683883597 PASS for pinned Nix derivation, SBOM and non-admitted bundles; attestation was skipped and is not claimed.

The CI source package for 0afad4 is preserved at `docs/effects/backup/semwright-effect-conformance-F-0afad4b.zip` with SHA-256 `81dce5784b01693f1c221e65937c25d4640ecea1a4c74aa384451325bf591b8f`.

## Readiness flags
EFFECT_READY_FOR_CONSUMERS=true.
EFFECT_READY_FOR_INTEGRATION=false: F01 owner A approval is missing and F12 is partial.
NATIVE_ACCEPTED=true only for the bounded F Godot/Blender workflows at the exact tested SHA.
SECURITY_CERTIFIED=false.
R16_CLOSED=false.
