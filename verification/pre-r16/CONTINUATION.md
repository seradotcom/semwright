# Pre-R16 continuation — independently tracked audit lane

The original audit head `495df3283f6e591c2db1a2448a7192e41335c580` completed
34 successful PR checks; the non-PR attestation job was intentionally skipped.
That evidence remains valid for that head. It is not transferred to subsequent commits.
The next observed main is `be375a12e8afa4d779f9dc0de501b0d4a262a682`, which adds PR153.

## Own remediation PRE-010: unfinished recording retention

Broker session revocation removed refs and jobs but not the corresponding active workflow
recording. Thirty-two expired sessions with unfinished recordings could occupy all recording
slots until daemon restart. This is bounded retention and recording unavailability, not proof
of cross-user authority escalation. The fix discards only the revoked session's unfinished
recording, without persisting partial traces or deleting completed owner-library entries.

Three WorkflowManager regressions cover isolation/idempotence, 256 repeated recording cycles
and persistence integrity. A broker regression executes 64 start/revoke cycles through the
normal workflow command and verifies that completed library traces are retained.
Rust execution evidence is supplied by the exact-commit Quality/Windows workflows, not by
this source description. No test is disabled, no limit raised and no owner platform edited.

## Own precheck: committed-source and history secret scanning

The hosted `Maintainer secret precheck` uses checksum-pinned Gitleaks 8.30.1, a positive
synthetic-token self-test, a Git archive of the recorded SHA and complete HEAD ancestry.
It publishes only finding locations/rule IDs/commit IDs, never matched bodies or credentials.
Unexpected scanner exits, missing reports, shallow history or zero-content inputs fail closed.
It does not scan unrelated unpushed worktrees, encoded payloads or nested binary archives and
does not replace independent security review. Findings require explicit triage.

## Ownership and candidate boundary

The Windows agent owns native UIA failure analysis, dedicated interactive-runner targeting and
positive executed-test admission. The Blender/Godot agent owns sealed-runtime/export integration
and its current checks. New dependency upgrades and demo expansion do not automatically become
required pre-R16 work. A change of main does not erase older SHA-scoped findings or proofs.
The audit lane continues independently; no new candidate SHA or independent review conclusion
is asserted here. R16 remains OPEN and global readiness remains NOT_READY until final admission.

## PRE-011: main-thread Figma codec assumptions

The production TypeScript called browser globals (`atob`, `btoa`, `TextEncoder`) while
its VM fixture injected those globals. The official Plugin API documents a minimal
main-thread sandbox and native `figma.base64Encode`/`figma.base64Decode`; iframe globals
must not be assumed to exist in that sandbox. Artifact encoding/decoding now uses those
native methods. A small bounded UTF-8 helper preserves byte budgets and lone-surrogate
replacement without DOM APIs. The fixture no longer supplies browser codec globals.
Added regressions cover Unicode/surrogates, artifact roundtrip and the 100,000-byte
plugin-data limit. This is a source/harness compatibility correction, not a new claim
of live Figma certification. No capability, manifest permission or authoring feature changed.

Primary API references, consulted 2026-09-28:
- https://developers.figma.com/docs/plugins/how-plugins-run/
- https://developers.figma.com/docs/plugins/api/figma/#base64encodedata-uint8array-string

## Secret precheck first execution and exact-content triage

Actions run 36465457336 executed the positive control and scanned 1,032 commits / 1,070
tracked files at PR merge-test SHA 989c4686c14f0b244e814d268654b91be6326256 (PR head
1fff19e90df9bc83c3d855ce47d5bf63fab1126b). It correctly returned FINDINGS, not a false PASS.
Three tree matches and their three historical introductions were manually classified:
public Figma REST API-spec commit identity; OBS fixture-only golden authentication response;
and the Godot Key game object's collision-shape resource path. The OBS golden regression
recomputes the response from a literal fixture password. No real credential was established.

`pre-r16-secret-triage.json` binds each exception to its rule, exact file and SHA-256 of the
single matching source line. Changed content, a different rule/path or a multiline match
remains untriaged and fails the gate. Both raw finding counts and explicit non-secret
classifications remain in evidence. This is not a directory exclusion or a zero-findings claim.

## Documentation-only false-positive follow-up

Run 36469051449 on document head 0e7c8bb917b0a5afecc15c413e99aae91b35b587 reported one
additional generic-api-key match: the scanner merge-test SHA under a metadata field named
secret_precheck_merge_sha. It exactly equals the already published source_sha receipt, not
a credential. Current metadata calls the field scan_source_revision; the precise historical
line remains content-hash/rule/path triaged so history is preserved rather than rewritten.
No file-wide exclusion is added and changed values still fail the gate.

The workflow recording fix now has all four named regressions executed in the successful
ARM64 Quality job 109082539796 (run 36467811798, code head 536e8b14). The Figma job
109082185324 executed both new codec regressions inside 89 plugin tests plus typecheck/build.
Later documentation or triage-only commits still receive their own checks.
