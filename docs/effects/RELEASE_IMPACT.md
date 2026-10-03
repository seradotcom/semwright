# Release impact — F

No release, tag, main merge, protection change or R16 closure is authorized by this role.

Added package: `semwright-effect-conformance`, inheriting workspace version/license/toolchain. Runtime dependency direction is F -> A semantic-composition; project-graph is dev-only. No duplicate EffectClass, Finding, VerificationReport, clock, vault or policy engine was introduced.
A C0 `26602e4...` and C P0 `6ee52b4...` are pinned consumed dependencies. Shared A wire/canonical formats remain unchanged. F schemas are additive and versioned.

Implementation source `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` has:
- F release 36942492444 PASS on Linux/Windows/macOS; Linux native Godot/Blender, Forbidden-obligation regression and mutations PASS.
- Current Quality 36942497243 PASS across source-contracts, static-lints, x86_64, ARM64 and MSRV.
- Current dependency/coverage/fuzz 36942497359: coverage PASS and fuzz PASS; dependency job FAILS only because the current RustSec database marks unchanged lock entry `yoke-derive 0.8.3` as yanked. The same lock passed cargo-audit/cargo-deny in 36683883572; d2cfd86 changes only a regression test. Effect Conformance does not rewrite the global lockfile.
- Current Packaging 36942497427 PASS including deterministic package and user install lifecycle on x86_64/aarch64 Linux.
- Current Supply-chain 36942497317 PASS: x86_64/aarch64 release bundles and pinned Nix derivation succeeded; attestation was skipped and is not claimed.

The CI source package for `d2cfd86` is preserved at `docs/effects/backup/semwright-effect-conformance-F-d2cfd86.zip` with SHA-256 `31536b484a3fc9e54fce7d3c570fa41440a1ccb043e4313b13c078061342f542`.

## Readiness flags
EFFECT_READY_FOR_CONSUMERS=true.
EFFECT_READY_FOR_INTEGRATION=false at this historical checkpoint: the initial review dependency is closed and the Blender consumer is exact-head green; the remaining consumer gate stays partial until the production audio consumer publishes and certifies the Effect Conformance revision it uses. The recorded repo-wide dependency audit was also red on a yanked lock entry and required integration-level reconciliation.
NATIVE_ACCEPTED=true only for the bounded F Godot/Blender workflows at the exact tested SHA.
SECURITY_CERTIFIED=false.
R16_CLOSED=false.
