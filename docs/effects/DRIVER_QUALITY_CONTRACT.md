# Driver Quality Contract — workflow-scoped dimensions

`workflow_quality` produces nine orthogonal dimensions, each using A's Verdict and evaluated RuleResults. There is no score, capability-count ranking or official trust badge. The identity includes driver/version/runtime/OS/workflow/fixture/source SHA and distinguishes contractual, native-adapter and native-Broker evidence.

| Dimension | Required observation |
|---|---|
| Actuation | Requested result was observed; acknowledgment alone is insufficient |
| Observation | Actual typed values/projections from the declared method, not request echo |
| Roundtrip | Comparable projections before and after the workflow |
| Persistence | Saved bytes and semantically equivalent observation in a fresh process |
| Effect-bounded scope | Declared excluded resources/inventory preserved within explicit bounds |
| Enumeration completeness | Consistent declared universe, valid complete pages and counts |
| Recovery | Executed fault/recovery proof; happy-path reopen is not recovery |
| Conformance | Required checks plus executed negative cases that reject mutants |
| Native evidence | Real native path for the exact identity; contractual consumers stay UNKNOWN |

Missing mappings remain UNKNOWN. Conformance without negative-case receipts remains UNKNOWN. A native-adapter result does not become a Broker E2E result. Quality is not inherited across applications, versions, operating systems or workflows.

The read-after-write route may be a native property, graph projection, independently guaranteed event, file/reopen, decoder or a declared observability gap. No symmetric tool-per-mutation requirement is imposed. An irreversible action without suitable readback is still an action, but it is not semantic VERIFIED.

The Godot probe exercises scene-only SaveOps, native ReadbackOps, fresh reopen, external material/animation bytes and a bounded inventory. The Blender probe exercises fixed Commands authoring, collection-limited GLB, fresh .blend reopen, decoded GLB node membership and receipt-vs-file digest. Three cases per backend include positive and targeted negative probes. Results remain unconfirmed until their exact-SHA Actions run succeeds.

Current gaps: full production D/E adapter integration, native enumeration of mutable multi-page app state, crash/fault recovery, generated accepted matrix, clean installation and platform acceptance. Recovery must not be marked PASS from the current happy-path fixtures.
