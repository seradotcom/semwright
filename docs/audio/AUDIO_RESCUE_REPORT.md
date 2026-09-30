# Audio source rescue and port

Baseline: `93f70241e9fb9d4c99ca76fab55c8517574a9f6f`.
Composition C0 consumed by normal Git merge: `26602e4b25929be869d69ef28fef4dd9713180d7`.

97 eligible files / 692600 source bytes were preserved from three original audio worktrees.
The private rescue manifest SHA-256 is `19c10b86bad74f38751f7c33b42229952ad71936af14809532e88663e522042f`.
Each file was read twice before copying; original worktrees remain untouched.
`audio-domain-professional` at `13b486aa67f89c039bc526321cccd869d1a68bd8` is the initial port source.
Other variants at `3a048cdde7f531811b7ffb7e126ad24346d6cd3a` and `a403114a6f5072dcaaa89453780b5d1f884704f9` are preserved for comparison, not merged wholesale.

The earlier recovery ZIP contained zero source files and is not an implementation source.
Old Cargo.lock, runtime manifests, caches, binaries and unrelated branch history are not imported.
New workspace package records are added structurally to the modern lock; Actions must verify --locked.

## Initial disposition

- Audio model/edits/routing/presets/projections: recovered for review and adaptation.
- Faust translation: recovered; native compiler/render evidence must be regenerated.
- Ardour OSC/deep projection/fixed Lua adapter: recovered; current Host authority and native acceptance need verification.
- Runtime process execution, asset publication, session refs, authoring and analysis: port/hardening required, not accepted by copying.

Main quality gates passed at inspection. Native application integration had a pre-existing Chromium failure, run 36492485154 / job 109164432463. Audio work does not claim to resolve that unrelated failure.
