# Canonical workspace binding proposal r1

Base source: 4849b3c1dcee9b37438801b4264f59480bff9e40. Draft design; platform verification is recorded per source revision.


## Observed source boundary

Owner FilesystemGrant values are validated by DriverHost and selected by manifest.mount.root.
Both sandbox_spec and sandbox_spec_windows set logical_name to that exact root and source to
its owner grant path. Original DriverProvider did not retain a public effective-root capsule.
Provider status reports identity/route/generation/interfaces and InvocationProvenance reports
provider/descriptor/catalog/generation; neither reports the effective mount root or a live token.
Two configurations with the same logical root and provider descriptor but different physical
FilesystemGrant.path values can therefore have the same public status/provenance shape. This is
a static source counterexample, not an executed installed-runtime test.

The original Linux launcher passes workspace source paths to bubblewrap --bind/--ro-bind;
it does not retain workspace source descriptors. Existing pidfd control does pin the actual
bubblewrap-reported child (not the supervisor). This proposal captures the actual mounted root
through that child's /proc/PID/root AFTER the real DriverHost Hello/capability/health handshake.
It compares the mounted instance to the owner source, retains the instance descriptor and a
pidfd, and revalidates both paths and the process before admission. Denied /proc access fails
closed. The two physical-root unit tests do not substitute for real sandbox/IPC acceptance.

## Public transport and trust boundary

1. providers.mount.observe takes provider and root, applies canonical filesystem.read policy,
   and returns a session-scoped typed opaque reference plus actual mount facts.
2. providers.execute_bound takes that reference and one original direct driver request. The
   wrapper itself requires the canonical providers.execute_bound capability. Its public risk
   is conservatively Mutating and its idempotency is NonIdempotent. Original command policy,
   confirmation, gate, audit and uncertain outcome remain authoritative.
3. Ordinary provider output may materialize only NativeTarget references, never privileged
   provider-mount references. RefStore retains one session/TTL/capacity/revoke lifecycle.
4. Only mount_driver_provider(Arc<DriverProvider>) attaches real platform capsules. Ordinary
   provider registration has no mount authority. A failed capsule does not disable ordinary
   provider behavior; mount observation remains unavailable.
5. Each admission fence re-resolves the typed token and current catalog Arc, plus connection,
   generation, provider/process lifetime and physical root. Ordinary refresh also cancels a
   separate binding_epoch under catalog publication; existing unbound epoch behavior stays.
6. Mid-effect invalidation returns UNKNOWN, without retry. Canonical child audit records the
   exact outer request id, original child id and original provider provenance before dispatch.
   Successful delayed results cannot refresh current admission. Dry-run claims no confirmation.

The response path and read/write/execute flags are facts about sandbox configuration. They are
NOT a local-write capability, actor ACL, approval, signing identity, or safe path-based staging
permission. No operation should infer authorization from these booleans. The binding constrains
an already-authorized original command; it does not grant that command new permissions.

## Consumer proposal and explicit limits

The intended remote consumer is Motionwright's native ProductionClient through the existing
validated executable/socket/session connection. It needs explicit support for the core wrapper
in expected_authority, outer command/provenance verification, then independent validation of the
successful nested original envelope and exact provider/descriptor/generation. Errors remain
errors; audit correlation preserves UNKNOWN custody, never triggers retry.

Graph first-authoring uses driver:manim-community and manim-runtime, with its existing fixed
profile (seven fonts and two license pins). MotionCanvas custom font staging uses
motion-canvas/media and is a different scope. A media binding cannot unlock Graph readiness.
The actual native profile observation/validation must accompany the Graph consumer.

The local Tauri StudioService semantic_graph_context/apply paths presently have no retained
canonical live Host guard. A remote observation/ref passed through JSON does not guard a later
local SQLite commit. This proposal does not set them Ready. The integration must provide the
actual Host-owned lifetime/Project-stamp admission path, preserving the atomic commit boundary.

Local MotionCanvas staging through a returned PathBuf retains a check/use race. It stays blocked
until publication uses retained descriptor-relative root operations or equivalent canonical
Host publication, with matching provider dispatch. No local write permission is inferred here.

Windows/macOS use default Unsupported for workspace attestation. Windows already has native
mount identity/ACL guards in its launcher; the platform adapter must derive from that actual authority, not a caller
path/JSON snapshot. Full physical/Linux and installed Windows evidence remain external gates.

NativeSDK CallContext.expected and Host ResourceVersion binding already exist. This proposal
does not establish or refute a separate issued actor/project ACL. SEC07's real multi-principal,
cross-project/revocation matrix remains a distinct evidence requirement; a resource string is
not that authority.

## Verification and rollback

Proposed focused G0: types provider_mount_references and core provider_mounts::tests, plus the
existing provider_runtime and audit regression suites. Negative cases cover typed forgery,
session/TTL/revocation, canonical policy deny, recursive/core/route/dry-run substitution, root
replacement, same-Arc refresh, real prompt decline and revocation, and one-call UNKNOWN custody.

Required G1: Linux actual DriverProvider, sandbox mount capture, ordinary CLI/protocol Execute
on the same daemon session, normal mount access and replacement; provider refresh/revoke/exit;
full type/provenance validation. Required G2: actual native consumer/installed runtime and real
platform capsule acceptance. Mock capsules are not G1 or installed acceptance.

No test execution result is claimed by source generation. Record exact formatted source SHA,
commands, runner identity and limitations before proposing integration. Feature rollback must preserve a reader compatible with all retained audit state. Future consumers remain blocked
until the reviewed canonical SDK revision and real authority are available.


## Admission expiry and completion

Admission TTL and operation outcome are distinct. Before dispatch, an expired reference is a
known StaleReference with no effect. An already admitted operation retains a revocation state
in the same bounded RefStore; expired records keep their budget slot while an in-flight holder
exists. Explicit session/backend revocation or store drop marks that state before removal.

After an otherwise successful original command, completion ignores ONLY elapsed TTL. It still
checks the same stored state/session, current provider Arc/connection/generation/epoch, root
instance and canonical policy. Expiry alone preserves the known original nested envelope and
returns admission_current=false with admission_state=expired. This does not permit Ready or a
new operation. A current admission returns current/true. Explicit invalidation during possible
effects remains UNKNOWN, with no retry and with canonical parent/child audit custody.

The deadline-bound regression blocks one effect, waits for the exact stored expiry, forces
normal reference pruning, then releases once; its paired explicit-revocation case stays
UNKNOWN. Zero-TTL store tests cover holder retention, shared capacity, invalidation and drop.
These tests do not supply installed/native platform acceptance.

## Audit reader compatibility and recovery

The optional parent_request_id preserves the canonical reader's ability to read
and hash existing parentless rows. The supported reader profile must also retain
parent_request_id in typed Record serialization when present and include it in
the original hash calculation. Parent/child IDs, sequence, previous/hash,
provider provenance, and known or uncertain outcomes remain unchanged.

This reader floor applies to the active audit.jsonl, every retained rotated
segment, and retained backup evidence used for recovery. Rotation of linked rows
out of the active file does not lower that floor. Audit::open validates the
active filename only; audit.tail is a bounded metadata view. A successful open
of a parentless active file does not establish compatibility with the retained
directory or its recovery evidence.

Raw S3 (4849b3c1dcee9b37438801b4264f59480bff9e40) is not a supported same-state
rollback after any parent-linked rows have been written. If unrelated features
must be rolled back, the selected build must retain a specifically reviewed
compatible typed reader. Any backport is a new source/binary revision with its
own validation; a numerical package version alone does not establish that
compatibility. Rewriting rows, removing parent fields, recomputing original
hashes, deleting logs, or substituting fresh state does not preserve that
original journal and is not a compatible rollback.

Before any deployment transition, preserve the original active and retained
segments byte for byte, together with their hashes and source/binary identity.
Recovery restores that inventory under the existing owner-held state/audit and
single runtime writer. Continue the original sequence/previous/hash chain;
do not create a second journal, Broker, or permission authority. Correlation
metadata is not an actor/project grant or an acceptance signal. Hash integrity
retains the journal's documented limit against a process with the same user ID.

This is a deployment compatibility requirement, not an implemented automatic
updater barrier. Release, updater, and runtime selection must identify how the
compatible reader is enforced before installed rollout. Selection of a real
compatible rollback binary and platform verification remain deployment gates;
this proposal does not invent an existing enforcement mechanism.

The existing fixed legacy fixture remains unchanged. The retained-history
regression uses the real Audit scopes and rotation, restores the complete
byte-identical inventory, and reopens/appends without changing prior bytes.
Disposable segment copies are checked by the same canonical Audit::open reader,
including changed and removed parent fields with the original hashes retained.
That test does not add an archive validator, prove that an older binary is
compatible, or establish automatic deployment selection. Record its result
against the exact reviewed source revision.

## Workflow recording compatibility

The mount observation token and bound invocation are ephemeral admissions. They are not replayable workflow steps. A bound invocation invalidates an active learning session both on entry and on completion, including a recording started while the provider was in flight. The wrapper, nested command, and observation are excluded from workflow capture, while canonical audit retains the original nested command, provider provenance, parent request linkage, and known or uncertain outcome. Ordinary unbound workflow recording is unchanged. This restriction does not rewrite manually authored jobs or recipes; they still enter the same Broker policy and NonIdempotent retry rules.
