# B/F12 integration consumer

Status: implementation awaiting exact-SHA Actions evidence; F12 is not yet certified.

The compiled audio planner creates F's seven-rule effect contract and pins its
digest before preparing A's plan. Client `effects.contract` dependencies are
rejected. The decoded-master address is pinned alongside mutation addresses.
Gain repairs build a fresh trusted contract for the new model and operation,
while retaining the original convergence budget and ledger.

AudioSession accepts only its driver's measurements bound to the owner and the
same provider/session/generation. Production verification uses the actual apply
request and attempt status through `validate_plan -> collect -> evaluate`.
The compiled adapter performs no I/O and grants no permission. Incomplete PCM,
missing required loudness and missing required true peak remain Unknown.

F's compiled readback has a fixed versioned method and no precommitted artifact:
a future render digest is unavailable at planning time. F's strict artifact
matching and independent oracles remain unchanged. The returned F report also
retains the original admitted decoder/version/artifact ObservationRef as
additional provenance; this adds no PASS or claimed mutation observation.
Only decoded-master constraints are reported as observed. Native audio behavior
still requires the final candidate's Faust/analysis/Ardour and combined AV gates.

Source regressions cover client verifier injection, missing decoded scope,
decoder provenance, partial decode, unavailable loudness, foreign measurement
channels, verification before application, and repair contract re-pinning.
The existing lifecycle tests remain in the suite. Fixture PCM is source-test
evidence only and never substitutes for final native certification.
