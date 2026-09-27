# Compile and verify

Compile only repeated traces that represent the same intent and operation sequence.

Parameter hints use a name plus step/JSON-pointer location. Parameterize values that truly vary between successful runs. Opaque refs usually need to be derived from a prior step or supplied explicitly; they should not be frozen as durable literals.

`workflow.verify` checks descriptor drift and validates the generated Recipe against the current Broker catalog. Static verification does not prove the workflow is semantically correct for every future input.

A live replay still goes through the same Broker policy and providers. Unknown-outcome mutations require state verification before another replay.
