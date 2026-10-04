# Project Graph threat model

## Assets and trust boundaries
Protected assets are durable logical identity, immutable revision/receipt history, current knowledge state, private project metadata, query visibility and rebuild reservations. Native application files remain owned by their applications/filesystem grants; Project Graph does not acquire blanket ownership of them.

Trusted boundaries are: authenticated local transport -> Broker policy/context; registered provider receipt/revision adapters; the Composition PlanVault/controller; private GraphStore directory; and scoped filesystem/native resolvers supplied by platform or native application integrations. Agent JSON, imported manifests, RevisionCandidate/ExecutionReceipt wire data, Skill text, locators, content digests, application names and caller-selected IDs are untrusted data.

The graph must preserve distinctions between logical identity, native identity and ephemeral refs. A path, filename, digest, UUID from another owner, session ticket or PID is never promoted to durable ownership.

## Principal threats
**Cross-owner disclosure.** Guessing an opaque ID, cursor or receipt must not reveal names, counts, existence or provenance outside current grants. Queries filter visibility before result construction; cursors bind owner/session, visibility and grant fingerprint.

**Evidence forgery.** A syntactically valid ExecutionReceipt, RevisionCandidate or EvidenceSource supplied by a client must not certify execution or native observation. Only registered host ReceiptAdapter/RevisionAdapter admission can create AdmittedReceipt/AdmittedRevision; RevisionAdapter assigns the durable revision and creates no activity edge. Verification remains a separate Composition/Effect Conformance verdict.

**Stale identity / path replacement.** Reusing a locator after rename/replacement must not silently reuse logical identity or old derivations. Binding generation and native instance evidence force explicit rebind/reconcile; same bytes after replacement do not prove continuity.

**Dependency omission.** Missing fonts, textures, import settings, runtime, descriptor, contract or other determining inputs must not produce cache-safe CURRENT. Coverage tracks incomplete extraction and unknown frontier.

**Rebuild authority escalation.** A stored plan must not become a command or grant. RebuildCatalog is trusted lookup data, proposals are bounded data, A PlanVault binds canonical bytes/session/grants/snapshot/epoch, and every real operation must re-enter Broker policy.
## Store and recovery threats
**Torn/corrupt local state.** Canonical journal rows are hash chained and sequence checked; materialized indexes are rebuildable only after canonical log verification. Transaction fault injection covers multiple commit boundaries. Commit acknowledgment uncertainty poisons the handle until reopen.

**Cross-app atomicity illusion.** A committed graph transaction does not mean Blender/Godot/file/AV side effects committed. The graph contains a durable PREPARED/APPLYING/terminal intent ledger and converts interrupted APPLYING to UNKNOWN; native host wiring around actual provider calls is still open. Unknown native outcomes are never synthesized as success or blindly retried.

**Malicious portable import.** Imported manifests cannot import locators, owner identity, trusted receipts, CURRENT state or permissions. Local IDs are reallocated and only bounded declarations are accepted.

**Destructive cleanup.** Tombstone is graph-only. `project.gc.preview`/`collect` are bounded to tombstoned graph-private records with zero revision/receipt/edge/intent references and preserve the canonical journal. They accept no deletion path and never unlink user sources or outputs. Removing one project's materialized record is not a claim of global ownership over a native resource that another project may also reference. Any future destructive native cleanup or shared-resource reclamation remains a separate policy-gated capability with explicit reference accounting.

## Denial, ambiguity and resource attacks
Denied/offline/timeout/ambiguous probes remain UNKNOWN, never MISSING. Watch loss/overflow/reorder invalidates affected scope until a bounded rescan. Node/edge/depth/result/cursor/store/payload budgets return truncation/conflict/resource exhaustion rather than fabricated completeness.

SQLite, canonical JSON, manifests and receipts have explicit row/byte/count limits. Fuzz targets cover manifest, receipt, traversal and synthetic store paths; their result is parser/state-machine evidence, not native security certification.

## Current blockers and non-claims
The candidate exposes bounded graph-state routes through Broker core dispatch and derives durable ownership from the authenticated OS user, while retaining ephemeral sessions for cursors/request evidence. It deliberately exposes neither client receipt admission nor arbitrary rebuild execution. A root read grant alone cannot mutate graph state; `project.manage` is independently required. Root boundaries use resolved paths so symlink retargeting fails closed.

Native projectors, Blender→GLB→Godot acceptance, Composition/Audio AV receipt flow and application-changing ExternalIntent dispatch/reconciliation remain open. Linux confined file observation is best-effort revalidation, not CAS. No distributed ACID, exactly-once execution, universal watcher, hidden background sync, destructive GC, security certification or R16 closure is claimed.
