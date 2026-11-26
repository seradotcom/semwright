# Bound applied Motion Canvas source readback

Use the existing owner-mounted Driver Host project and Broker session. This
additive read does not change the plan/apply/inspect contracts. It does not
authorize filesystem access by a returned path.

1. Plan and apply a Film through the existing composition protocol. Retain the
   completed apply's plan_ref, fingerprint and execution provenance
   provider_generation. A dry-run or planned-only receipt is not an apply.
2. In the same Broker session observe the genuine project mount using
   providers.mount.observe with provider driver:motion-canvas and root project.
   Require its provider_generation to match the retained apply provenance.
3. Use providers.execute_bound with that binding_ref and the nested command
   driver.motion-canvas.composition.source.read, args plan_ref,
   expected_fingerprint and expected_provider_generation, dry_run false.
4. Require the bound envelope to report admission_current true, the original
   binding and nested provider provenance for the retained generation. Decode
   the standard base64 source_base64, verify byte_count and SHA-256 against
   fingerprint, and retain those exact bytes. Never reconstruct this file from
   Film or from project.inspect.

The driver reads only the actual committed semwright-motion.json in its
current managed project or managed island. Its fixed logical resource is
project and relative_source is either semwright-motion.json or
.semwright/motion/semwright-motion.json. No caller path, media/output grant,
extra network access, render, write or new authority is introduced.
The source is bounded to 512 KiB before encoding. The driver requires the
applied host-owned plan, original project identity and current source hash.
Core requires the genuine current project mount admission and expected live
provider generation before dispatch and rechecks admission at disclosure.
Core retains a bounded receipt for each successful, audited apply on its
actual immutable provider lease. Refresh/reconnect does not inherit those
receipts. A caller cannot relabel an old apply by asserting a new generation.
A new provider process also does not inherit plans. Session revocation
discards retained source receipts. Retention failure does not alter legacy
apply success, but it disables this readback for the missing receipt.
Stale or revoked mounts, foreign sessions, unapplied plans, changed bytes,
unsupported source and oversized input fail closed. Refreshing discovery does
not itself authorize a read of an old applied receipt.

The hosted source-readback-only lane compares the returned bytes with the
physical original file and verifies rejection of invalid mount, owner,
fingerprint, generation, path and lifecycle cases. It does not certify
rendering, any downstream hybrid pipeline or a release.
