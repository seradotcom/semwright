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
