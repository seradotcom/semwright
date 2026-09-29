# Project Graph security delta

## Authority
C consumes A C0 Owner, resource/base/observation/report, canonical digest and PlanVault. Public JSON is data, not permission. ProjectAccess, ReceiptAdapter registration and receipt admission are trusted-host APIs, not network requests. AdmittedReceipt has no Deserialize implementation. IDs and content hashes do not grant access. Native observations must come through an authenticated adapter under current grants; a caller-controlled EvidenceSource field is not evidence of that origin.

The current component has NO registered Broker Project Graph route. The proposed adapter is preserved in `integration-drafts/project_graph.rs` for review only. The attempted global Broker/dependency/daemon wiring command was rejected by the remote tool and was not applied. The draft is not compiled or accepted. No alternate tool or delegation was used to bypass that block.

A second blocker is deliberate: the daemon authenticates local peers before handing requests to the Broker, while Broker session IDs are restart-scoped. Unix has a kernel UID at the transport boundary and Windows validates a pipe peer process, but C has no existing cross-platform durable-principal value propagated into request context. Persisting the session ticket, PID or caller-provided name as ownership would violate the identity contract, so runtime graph ownership remains disabled until that trusted principal boundary is reviewed.

## Data isolation
Queries filter actual visible identities before returning names, counts, impact and cursors. Cursors bind principal/session, project, actual visibility, grant fingerprint, query digest, graph revision and observation epoch, with bounded lifetime and capacity. Raw historical receipts/revisions and full backups require full-project access because nested provenance can disclose other inputs. Portable exports omit native locators, absolute machine paths, authentication owners, parameters and acquired evidence; imported records get new local identities and declared edges only.

## Durable storage
The optional store uses a private owner/project directory, bounded SQLite rollback journal, synchronous EXTRA, a hash-chained canonical event journal and transactional materialized indexes. Unknown schema versions, journal corruption and commit uncertainty fail closed while preserving evidence. Index repair is explicit and rebuilds only from a valid canonical journal. Backups have one bounded non-overwriting slot; restore writes a new directory, never replaces live state. `ExternalIntent` records preserve the dispatch boundary without becoming an execution grant: APPLYING from an older observation epoch is exposed as UNKNOWN and cannot be redispatched; COMPLETED requires a matching already-admitted receipt. Hashes detect accidental corruption, not malicious rewriting by an actor already able to modify all private state.

## Observation and continuity
Existence, freshness, divergence, verification and coverage remain independent. Denial/offline/ambiguity are UNKNOWN. A watch notification is not permission to read a source; registration and event gaps invalidate scope until new observations. Restart resets live observation evidence. Matching bytes after an explicit rebind do not validate old generation-dependent receipts. Evidence for a different base/output is rejected or remains UNKNOWN.

The additive Linux filesystem primitive uses the existing openat2 confinement, a pinned descriptor, regular single-link files, bounded reads, birth-time instance evidence and post-read descriptor/path revalidation. It is best effort, not CAS or a proof against every concurrent writer. Unsupported birth-time/platform methods fail closed. Other platforms retain the default Unsupported observation method.

## Explicit limits
No native D/E or AV/audio receipt adapter is integrated. No graph-side watcher daemon, native wiring of the external-intent ledger around Broker/provider dispatch, distributed ACID, exactly-once execution, automatic destructive GC or native rebuild dispatch is claimed. Store phase-fault tests are transaction-boundary injection, not an exhaustive OS/power-loss campaign. Native file tests exercise only the confined filesystem primitive, not Broker or Blender/Godot/AV E2E. R16 remains open; this document and an independent agent are not security certification.
