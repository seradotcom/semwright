# Godot authoring coverage

This is an implementation/evidence map, not a completion score. GODOT_AUTHORING_READY=false until the exact current SHA has native persistence/export/hostile/package evidence and remaining cross-app obligations are closed.

| Workflow / capability | Implementation / native API | Authorization | Observation / persistence | Evidence state |
|---|---|---|---|---|
| Composition authoring | composition inspect/plan/apply/measure/validate/repair/verify over A PreparedPlan and PlanVault | authenticated Driver Host owner; grant-bounded Store | per-file hashes, C logical IDs, F source effect evaluation, C apply receipt | implemented; current-SHA CI pending |
| Empty-root bootstrap | Store prepare/apply creates only declared managed files | output/state/input grants; no caller path | plan creates no target bytes; apply journal/revision read back | Broker/Host E2E implemented; native lane queued |
| 2D/3D + Behavior IR | typed Nodes/Resources/scenes, expression DAG, state/events/actions, generated GDScript | no run_code, method strings, plugins, executable paths or shell in intent | native observer reads actual Godot nodes/resources/signals/animations/runtime telemetry | source suite passed at 44adb06; current native E2E pending |
| Native verification | composition.native.verify with plan_id and typed inspect/persistence/play scenario | CodeExecution; advertised only with authoring + pinned runner; A plan/session resolved server-side | verified private-copy import, owner/request/project/plan/intent/source binding, fixed observer | implementation current; exact-SHA native evidence pending |
| Animation paging | full AnimationPlayer libraries/tracks/keys; snapshot/source cursor | verification plan binding | no silent last-track truncation; page size 1..64 | portable regression passed; live paging pending |
| Save/reopen | fixed observer save then reopen in separate processes | native verification capability | F Reopened value; complete external dependency sentinels must match | portable regression passed; native lane pending |
| Managed run/export | existing validate/run/export routes accept logical managed_project | runner + authoring grant; Store must be IN_SYNC | verified sources copied to disposable project before import/run/export | standalone export/run E2E implemented; evidence pending |
| Artifact / GLB intake | composition.plan advertises artifact-in:model/3d; Store validates digest and self-contained GLB v2; Instance realizes PackedScene | generic Broker artifact.handoff into granted input root | private-copy Godot import can observe meshes/materials/skeletons/animations/dependencies | public D consumer boundary implemented; actual E GLB run pending |
| Human drift / repair | IN_SYNC, DIVERGED, PARTIAL; missing-source repair only | original A budget and owner-bound plan | external edit blocks replan/apply/run/export | source tests passed; final host gate pending |
| Export hygiene | export preset excludes addons, authoring spec/manifest/source maps; private probes never enter canonical project | existing runner/export policy | exported executable launched without editor/Semwright and scanned for private markers | implemented; exact-SHA native evidence pending |
| Skill/package | semwright-godot-production; deterministic Git-object source pack | production Skill requires native/run/export routes; package waits on model+native jobs | ZIP has patch, manifest, hashes and reconstruction proof | Skill committed; package job prepared, not yet evidenced |

Open work remains explicit: managed islands in arbitrary human projects, an actual E-produced Blender GLB roundtrip with cross-app C receipts, native F effect admission for all runtime predicates, hostile/fuzz closure and final exact-SHA packaging.
