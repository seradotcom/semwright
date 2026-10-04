# Audio / Effect Conformance integration consumer

Status: source implementation passed Actions run `37006551930` at
`6d107e8700b97d6ccfe8930a33141d418718e2d5`; final native consumer certification remains pending.

The compiled audio planner creates the seven-rule Effect Conformance contract and pins its
digest before preparing the Composition plan. Client `effects.contract` dependencies are
rejected. The decoded-master address is pinned alongside mutation addresses.
Gain repairs build a fresh trusted contract for the new model and operation,
while retaining the original convergence budget and ledger.

AudioSession accepts only its driver's measurements bound to the owner and the
same provider/session/generation. Production verification uses the actual apply
request and attempt status through `validate_plan -> collect -> evaluate`.
The compiled adapter performs no I/O and grants no permission. Incomplete PCM,
missing required loudness and missing required true peak remain Unknown.

The compiled Effect Conformance readback has a fixed versioned method and no precommitted artifact:
a future render digest is unavailable at planning time. Strict Effect Conformance artifact
matching and independent oracles remain unchanged. The returned Effect Conformance report also
retains the original admitted decoder/version/artifact ObservationRef as
additional provenance; this adds no PASS or claimed mutation observation.
Only decoded-master constraints are reported as observed. Native audio behavior
still requires the final candidate's Faust/analysis/Ardour and combined AV gates.

Source regressions cover client verifier injection, decoded scope pinning,
decoder provenance, partial decode, unavailable loudness, foreign measurement
channels, verification before application, and repair contract re-pinning.
The existing lifecycle tests remain in the suite. Fixture PCM is source-test
evidence only and never substitutes for final native certification.
