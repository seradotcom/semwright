# Follow-up findings after initial reconstruction

## Historical Windows network backup

`backup/pr132-pre-rebase` at `3f087430aa006889a9206a5121efa654f32118af` is behaviorally
superseded by the initial main. All three changed paths retain the intended internetClient
opt-in behavior and both native assertions; main adds profile/LPAC and token-buffer hardening.
The branch/worktree remains KEEP. Of the initially unresolved 48 distinct heads, one is now
resolved by direct behavior/source/test comparison; 47 still require disposition. The original
raw comparison inventory is intentionally preserved. See `inventory/behavioral-dispositions.json`.

## Blender registry root cause

The exact PR154 head `d46b241b97beb14093da71582c5106e6d616271c` has 143 builtin descriptors
but 142 explicit provenance records. `blender.export.glb` is missing from
`schemas/builtin-provenance.json`. Quality run 36397956332 fails Registry::builtin on MSRV and
both Linux architectures; Python also reports the stale 142-count expectation. This is not a
reason to infer trusted provenance or skip a failing test. The owner received comment 5866922131.
The audit adds a separate source-only parity regression so this fault is directly reported before
Rust compilation. The new test passed against the audit's unchanged 142-command main catalog.

## Windows fixture reproduction

PR156 head `4c7c296a7f58124f0eead4319044985949fe18d9` reproduced the original ARM64
StaleReference assertion in run 36401539616/job 108860320846. The fixture function captures a
semantic snapshot before repeated physical-point waits, then returns the aged snapshot on its
occlusion path. This is a plausible cause of generation drift, not a demonstrated new broker
security bypass. The owner received comment 5866968429 with the suggested stable observation
boundary. No timeout, assertion or generation guard was weakened and no owner file was changed.

## Website correction

A read-only website inspection found bounded historical real Figma and Godot public proof records.
Their precise route/SHA limitations are preserved in WEBSITE_AND_PUBLIC_PROOFS.md and the root docs.
They do not turn the current audit head into a live-certified or independent-review baseline.
