# Frozen Windows native evidence — checkpoint01

This archive preserves retired technical candidates03–07. Read each original receipt: all full lifecycle fixtures in this archive failed, candidate04 cancellation passed, and candidate07 completed a read-only compatibility inventory. Archival verification does not change those results. Candidate08 is outside this frozen archive.

manifest.json maps 874 raw copied payloads (15,920,308 bytes) to their original paths and SHA-256. JSONL audit chains were verified independently per candidate from sequence 1. The 188 recorded request/envelope pairs matched broker command, provider/descriptor provenance, policy decision and outcome. Unmatched proof requests: zero. Additional audit records, including cancellation and cleanup, remain preserved rather than discarded.

The audit checks establish local consistency and correlation, not resistance to malicious same-owner rewriting. Configurations contain local paths, user SID and named pipes, but no client session ticket was copied. Host logs were copied as bytes into .txt files. The parent .gitattributes disables Git text conversion for this archive; staging must verify payload blob hashes against the manifest.

Build source directories are modified/untracked-source overlays, not full checkouts. The physically relevant build04,06,07,08,09 and test overlays use base commit 53989cd1debad07c22f6d8a64eaca29ee28a25d7; combine that base with the preserved overlay. Original run reports record source hashes before/after and compiler results. Earlier reports without an archived overlay remain historical reports only. No deleted source files were part of those overlays.

Runtime/executable inventories and exact candidate binary hashes are preserved; large binaries and the private 944,579,836-byte Blender runtime are not duplicated in Git. This is a local review checkpoint, not a published contribution or release. Linux native regression remains NOT_RUN; artistic acceptance remains REVISE.
