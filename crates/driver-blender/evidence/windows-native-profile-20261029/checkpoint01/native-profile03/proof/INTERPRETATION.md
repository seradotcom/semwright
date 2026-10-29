# Native profile 03 — bounded physical results, 2026-10-29 UTC

This is a technical fixture, not Archipelago art acceptance.

## Demonstrated

- Production broker/Policy/DriverProvider, one official `semwright_protocol::Client` connection with ticket held only in memory.
- Blender 5.2.1 LTS initial process: LPAC, exact Job before resume, exact one registryRead capability (attributes 4), production `test_only:false`; no network or GPU grant was requested. registryRead is not a SideBySide-only key whitelist.
- Typed collection/object creation, link and transform; independent typed readback: location `[3,4,5]`, scale `[1,2,1]`.
- Broker GLB export request `d0adac3bc2d343c892859e3586a8e51f`; artifact SHA256 `d0773ba083cea6f9697678f528f9e82e57ae7a118beadef61eb9bacfcb0f9c04`.
- Duplicate GLB export rejected; existing hash unchanged. Parent traversal rejected; outside target absent.
- The exact GLB was imported by native Godot 4.7.2 and rendered in a visible Windows viewport. One mesh, one surface, 24 vertices; global translation `[3,5,-4]`, scale `[1,1,2]`, local AABB size `[2,2,2]`.
- Native viewport PNG 1280x720: SHA256 `635c3a87f04b422f3c8dfacd4315fd98bd0b311e3978e7b823509b55f7aa0f43`. Godot reported NVIDIA GeForce GTX 1650 / GL Compatibility. This is not a performance benchmark.
- Godot was launched as a declared native consumer through Desktop Commander. `godot_broker_used:false`; Windows Godot driver qualification is not claimed.
- Owner gate negative attempt02: removing only the explicit profile opt-in produced PolicyDenied, exit 3, before native preparation/launch. Attempt01 failed earlier because its test state directory was missing and does not count as a policy proof.

## Still failing / unresolved

The aggregate `fixture-attempt01` result is **FAIL**, exit 101. `session.checkpoint_close` returned Unsupported; cleanup also failed. The broker redacted the untrusted error and marked its outcome unknown. No recovery .blend or successful native close is claimed. A read-only inventory is being used to identify the unsupported state before changing the contract. This failed run must remain preserved.

The private runtime snapshot took 105254 ms and contains 944579836 bytes, 7500 entries. Its inventory SHA256 is `d7c64dad43c854847e3ff241fbcb0201321aee4598fdf33de4d44db27a284205`. This is an owner-pinned inventory, not an official distribution signature or crash-durable cache.

## Gates

- Bounded broker → isolated Blender → typed mutation/readback → GLB → native Godot consumer: demonstrated by the linked raw receipts.
- Complete lifecycle/recovery fixture: FAIL, under repair.
- A05: PARTIAL; project-level conformance and the reviewable upstream change set remain unfinished.
- G6: NOT_COMPLETE; same-project revisions and preservation of human edits are still required.
- A02: REVISE; no artistic acceptance, no new art cycle in this proof.
- Linux 4.5.14 regression: NOT_RUN.

Evidence: `fixture-attempt01/`, `owner-gate-attempt02/`, `godot-consumer-attempt01/`, plus `../20261028-host-baseline/native-profile03-broker.stderr.log` under the integration evidence root.
