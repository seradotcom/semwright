# H evaluation laboratory — preparation

This branch changes the evaluation laboratory and its hosted harness workflow only.
It does not change the product, certify R16, select a winning arm, or execute model
sessions. The current implementation validates result identities, task coverage,
route declarations, missing costs, and paired all-attempt analysis.

`protocol.json` is a draft protocol. `tasks/public-dev.json` contains ten public
development instances across five families, each with creation and five revisions.
These are not final heldout tasks. Tests use explicitly synthetic evidence and a
synthetic model identity to exercise the validator; their output is recorded as
`DETERMINISTIC_HARNESS_TESTS`, never as productivity data.

Run harness checks on GitHub-hosted Actions through the workflow on this branch.
Do not run tests or native applications on the storage-constrained workstation.
The workflow checks out its immutable laboratory SHA and publishes the test log,
source identity and protocol/task digests. It installs no third-party dependencies.

Before final evaluation, complete competent direct helpers, independent native
oracles and model-session execution adapters; preregister their digests and common
model/runtime/Skills configuration. Provisioning may install declared engines and
assets but may not author hidden scene behavior. Collect route records outside the
model's writable output directory. Model-provided `native=true` or a JSON receipt
alone is not an independent native oracle.

Final execution also requires I's technical gate, a full immutable target SHA,
fresh heldout instances with a seal/reveal ledger, explicit comparable model access
and authorized budgets. No model API credentials are discovered or used here.
Current model evaluation is blocked until that configuration exists.

The preparatory integrator and harness author are currently the same assistant.
This is not an independent security review or blind aesthetic assessment. Heldout
parameters must be reserved procedurally and chosen before final measurement;
changing the target after exposure requires a new round and new heldouts.

All original failed runs and raw outcomes must remain available. Unknown costs
stay null with reasons. Direct code, modules, native APIs, tests and reusable tools
are allowed in the competent direct arm. Strict semantic coverage is a separate
study, rather than a restriction imposed on the direct baseline.

Foundation source3cf2b44092668a5728749686b78dbea59c9edeaa passed28
deterministic controls in GitHub run37074603304. That evidence does not certify
a complete native/model harness. The preparatory delivery contains the lab source,
public draft tasks, protocol, control logs and hashes; it contains no model results
or final heldouts. ZIP creation and reproducibility checks run only in Actions.

The received technical product target is now
`cd518748f742025a251b78028613aa1b16919e73`, bound to the immutable I manifest
in `frozen-target/` (SHA-256 `23ed96b04e1ac14aefb0a729e619d0cb1f5d4cea2a23a0079b1edf38989068c7`). Product checkout
and laboratory checkout are distinct: do not evaluate the old product ancestry
of this laboratory branch. The target is technically frozen; the model protocol
and evaluation suite remain draft/unfrozen, and no evaluation has run.
The original global failure and successful corrected ARM64 external-fixture
disposition remain explicit in the I handoff; they are not relabelled.
