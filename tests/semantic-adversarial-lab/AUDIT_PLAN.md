# Independent adversarial conformance lab

## Authority and immutable targets

This laboratory is an independent test surface, not product implementation or release authorization. Its scope is limited to `tests/semantic-adversarial-lab/` and the dedicated workflow; product code, permissions, secrets, billing and release state are outside the lab boundary.

Base: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
Composition contract snapshot: `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d` (PR 168).
Audio snapshot: `11b40fb0c59473bc0f007a879bf44c732d22b700`.
No integrated candidate has been supplied. Early snapshots are separate experiments, not a mosaic of evidence certifying a combined revision. The initial main inventory included a failed AT-SPI check; no baseline is declared universally green. Blender export PR 154 remained open at `74671c11dda2133ce6af939896c49cdbb6ba47d5` and is not presumed merged.

## Threat model

Protected assets: owner/session authority, declared write sets, execution budgets, observation integrity, project identities and dependency history, user-owned sources, private artifacts, native persistence, capability provenance, and truthful evidence. Experiments replace private data with synthetic canaries.

Untrusted actors/inputs: malicious capability clients; other sessions/principals; hostile metadata/documents; replaced plans/profiles/schemas/manifests/imports; forged reports; reconnected generations; late/duplicate/reordered events; files replaced during operations; active native content/plugins; incomplete dependency graphs; bounded resource pressure. Same user, loopback, official API, Rust, a typed schema, and a digest are not authority or confinement proofs.

Boundaries: authenticated Broker to policy/provider; planner to vault/reservation/side effect/receipt; application readback to verifier/publication; Project Graph to native identity/store; native Godot/Blender/Figma/Motion/audio/AV; packages/Skills to loader and grants; lab observations to reviewer.

## Experiment safety

Attacks run only on disposable GitHub-hosted Linux. Provisioning is separate from tested subprocesses. Tested processes get cleared environment, private HOME/tmp/output, PID/network namespaces, read-only runtime/source mounts, bounded wall time/CPU/address space/FD/file/output, and process-group cleanup. Independent preflight checks canary visibility, denied write, environment exclusion, and namespace separation. This proves the test enclosure, NOT the product sandbox.

Only synthetic canaries. No user files, real credentials, microphones/playback, external listeners, exfiltration, or tunnels. Failed isolation produces BLOCKED without weaker fallback. Source edits, Git/gh, JSON and syntax checks are local; cargo/npm/native apps/fuzz/mutation are remote only. The lab workstation footprint is capped at 256 MiB; do not delete other authors' data.

## Claims and independent oracles

G03: rehehashed expanded writes; owner/ref/profile/schema/rules substitution; root/child budgets; reservation before side effects; interruption at reservation/dispatch/mutation/receipt/publication; replay, unknown outcome, expiry/restart/revocation and generation reuse; cycles/no progress/ambiguous repair.

G04: fake PASS, class/source spoof, fixture labelled native, stale plan/base/artifact, missing final/duplicate checks, false exhaustive flags, invalid tolerances, expected-as-readback, optional-warning abuse. Require positive/negative controls and relevant guard mutants. AABB, sampled frames, loudness and memory are not mesh collision, whole-video quality, intelligibility or persistence.

G05: stable-mtime replacement, rename/copy ambiguity, incomplete links, offline versus missing, replay/gaps/reorder/duplicates, forged verified_by, cross-owner cursors, index corruption, canonical versus incremental graph propagation, legitimate/build cycles, depth/overload, crash/receipt reconciliation, GC/privacy.

G06/G07: Godot full properties/pagination/long animation, owner versus parent, shared/local resources, external-resource noninterference, fresh-process save/reopen, UID/reimport/active content/export, bounded typed behavior/codegen; Blender instancers/libraries/shared data/evaluated modifiers/negative scale/active content/export membership using independent GLB readback. Authoring starts empty through product routes; oracles may read but not author or repair the claimed outcome.

G08: Figma codec parity/collaborative drift/read-only writes/fonts/masks; Motion render identity/tamper/seek/partial/cold; audio nonfinite/silence/true-peak/malformed WAV/routing/auth/reconnect/cleanup; AV stage failure/stale cues/common drift/priming/padding/double processing/incomplete publication.

G09/G10: bounded lifecycle windows, growth versus leaks, cancellation/crash/lost response/quota/corrupt output; targeted fuzz/minimization; actual route/provenance/policy and no fallback escalation; package traversal/symlinks/binary hash/schema and no authority acquired by installation.

Contract probes exercise public APIs through declared test examples in a disposable build copy. The immutable checkout remains unchanged. Parent-side oracles compare typed results and complete case sets, never a generic PASS word. Synthetic NativeApi values remain product-contract evidence, not native acceptance. Mutant diffs are separate experiments, never product fixes.

## Findings and retests

Findings record stable ID, target/runtime, boundary/actor, class (bug/weakness/known unsupported/upstream/test defect/documentation overclaim), impact/reachability, claim, expected/actual, minimal synthetic reproduction, hashes, and prerequisites. Subsystem mapping: Composition/Figma/Motion/AV; Audio; Project Graph; Godot; Blender; Effect Conformance. Do not silently patch product code.

Sensitive unfixed details stay private; sanitized public reproduction requires coordination. Severity is not lowered for a deadline. Before/fix/after evidence and the affected family on FIX_SHA are mandatory for closure. Failed experiments remain in history. The final status declares tested and untested scope; no security guarantee, demonstration, or automatic R16 certification.
