> **Historical first-pass report.** Current continuation, owner assignments and admission status are in [R16_HANDOFF_READINESS.md](R16_HANDOFF_READINESS.md), [OWNER_HANDOFF.md](OWNER_HANDOFF.md), [BRANCH_DISPOSITION_CLOSEOUT.md](BRANCH_DISPOSITION_CLOSEOUT.md) and `pre-r16-audit.json`. Earlier snapshots and failed logs below are preserved, not presented as current-head certification.

# Claims-to-evidence matrix

Observation: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. Levels are not interchangeable: IMPLEMENTED, COMPILED, UNIT_TESTED,
HOSTED_INTEGRATION_TESTED, LIVE_TESTED, HARDWARE_TESTED, INDEPENDENTLY_REVIEWED.
No row is INDEPENDENTLY_REVIEWED. Historical records retain their own SHA.

| Claim / source | Evidence level supported | Evidence / correction | Limit |
|---|---|---|---|
| Linux core / Quality gates | COMPILED; UNIT_TESTED; HOSTED_INTEGRATION_TESTED | initial-SHA Quality gates success; protocol/broker fixtures | does not certify every desktop or later SHA |
| Provider/plugin/driver sandbox | IMPLEMENTED; HOSTED_INTEGRATION_TESTED | 3 plugin hostile + 1 driver hostile + 1 protocol-v2 tests actually ran | configured Linux boundary only, not kernel proof |
| MCP federation | IMPLEMENTED; HOSTED_INTEGRATION_TESTED | sandbox required in source; 7 real protocol-fixture tests ran | no remote transport/publisher/same-UID global isolation claim |
| Blender | IMPLEMENTED; HOSTED_INTEGRATION_TESTED | initial native job success; README no longer says mock-only | PR154 runtime/export changes are outside observed main and currently fail |
| PipeWire/EIS/AT-SPI deltas | IMPLEMENTED; hosted/historical route-specific evidence | remove obsolete doctor field and README deferred labels | synthetic stream/nested output is not physical monitor acceptance |
| Windows | IMPLEMENTED; partly COMPILED/UNIT_TESTED/HOSTED_INTEGRATION_TESTED | x64/compatibility success; ARM64 UIA failure retained | no overall green or interactive certification |
| macOS | IMPLEMENTED; native hosted build/noninteractive tests | initial Intel/ARM jobs success | no TCC/live desktop acceptance inferred |
| GNOME/Plasma isolated input | Historical LIVE_TESTED | dated VM records preserve delivery/cancellation/focus checks | not rerun on initial SHA; not physical R06 closure |
| Hyprland nested AMD path | Historical HARDWARE_TESTED within nested setup | preserved hardware-backed render-node record | synthetic second output is not physical mixed-scale |
| Figma | IMPLEMENTED; hosted protocol tests; historical limited LIVE_TESTED record | initial native/continuity jobs plus public proof at 3cd86958 | historical real driver/Plugin API proof is not full CLI/broker or all-surface acceptance |
| Godot / Motion / MLT / KiCad / OBS | Driver-specific implementation and test levels | separate workflow/source records; no common full-API claim | runtime and curated-surface coverage differs by driver |
| Jobs/events/Tasks | IMPLEMENTED; UNIT_TESTED; hosted conformance | remove obsolete follow-on progress/artifact wording | remote durable task persistence not implied |
| Workflow Distillation | IMPLEMENTED; contract tested in quality suite | record/replay/promote gates and descriptor drift checks | proposals do not grant authority; no formal information-flow proof |
| Distribution / packaging | HOSTED_INTEGRATION_TESTED | static package/companion validation; initial packaging success | hashes are integrity, not remote publisher identity |
| Nix/SBOM/attestation | HOSTED_INTEGRATION_TESTED | run 36394993370; scoped attestation job | no release/security closure or notarization inferred |
| All-green exact commit / VERIFY | NOT SUPPORTED at initial SHA | replaced evergreen verdict with recorded Windows failure | subsequent changes need their own gate results |
| Independent security review | NOT SUPPORTED | R16 OPEN; no reviewer report accepted | maintainer preflight must not close R16 |

## Artifact drift

The initial scan found 16 tracked verification JSON files: 8 historical, 2 explicitly invalidated,
6 without a recognized commit field, and zero exact-current records under the chosen parser.
This parser is conservative and field-based; unscoped files may contain useful evidence not captured
by its schema recognition. `inventory/evidence-inventory.json` records parsed fields and hashes.
These files are preserved; none is silently relabeled CURRENT or removed for containing a failure.

## Documentation remediation

README, VERIFY, compatibility and changelog now separate implementation and execution levels.
The historical September 21 changelog entry no longer implies the current development version lacks
its committed lockfile and later features. `RELEASE_BLOCKERS.md` still needs coordinated edits in
the Windows owner branch: its exact-document-SHA/green assertion and R11 sandbox sentence are stale.
The subsequent read-only website check is documented in WEBSITE_AND_PUBLIC_PROOFS.md. Its historical pin is deliberate; limited real Figma/Godot proofs are acknowledged without promoting them to current-SHA evidence.
