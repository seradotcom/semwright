# Release impact — F candidate

No release, tag, main merge, protection change or R16 closure is authorized by this work. The PR remains a draft until current-SHA acceptance and owner handoffs are complete.

Added package: semwright-effect-conformance, inheriting the existing workspace version/license/toolchain. Root members already use crates/*. The new production dependency direction is F -> A semantic-composition; C project-graph is a dev-dependency for receipt conformance, not a circular runtime dependency.

Dependencies consumed deliberately: A C0 26602e4b25929be869d69ef28fef4dd9713180d7 and C P0 6ee52b428310370d3ad438a13964086a63f48367. Shared C0 code at the observed A C1 tip was byte-identical. No A/C source was edited. The C merge's Cargo.toml conflict was resolved by retaining the additive project-graph workspace entry.

The effect schemas are new and versioned. A's shared enums, report wire format and semwright-json-v1 canonicalization remain unchanged. Consumers pin the effect contract digest; editing required checks, units or tolerance invalidates the prior binding. Schema additions need a deliberate consumer review, not silent use of latest branches.

The F workflow runs only its selected crate/lane. Native probes use the repository's pinned Godot 4.7.2 and Blender 4.5.14 runtime references and checksums, on ephemeral GitHub-hosted Linux. No local native runtime, Rust build/test, fuzz or mutation workload was executed.

Outstanding release gates include final current-SHA contract/native/mutation/fuzz evidence, Linux/Windows/macOS portable conformance, audit/license review, clean install/packaging, production D/E/A/B adapters and owner A approval. The bounded schema mutation fuzzer is explicitly not coverage-guided libFuzzer; no unmeasured coverage percentage is claimed.

## Ready flags
EFFECT_READY_FOR_CONSUMERS: E0 published for review, current evaluator API candidate only.
EFFECT_READY_FOR_INTEGRATION: false.
NATIVE_ACCEPTED: false (not yet confirmed).
R16_CLOSED: false.
