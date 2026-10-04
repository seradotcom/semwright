# Composition contract C0

Status: implementation published for consumer review; CI evidence belongs to the exact source SHA, not to this document. Composition owns these shared contracts; the audio subsystem owns audio-domain, Faust, Ardour and audio-authoring.

Use workspace dependencies `semwright-semantic-composition` and `semwright-media-time`. No private path dependencies. The core has no application, media backend, audio loudness, filesystem or Broker dependency. Domain intents/operations remain distinct types. `PreparedPlan<I,O>` contains the generic lifecycle envelope, never arbitrary executable strings. A profile is trusted compiled integration code; a descriptor supplied by a client cannot authorize itself.

Canonical JSON version is `semwright-json-v1`: UTF-8 strings are preserved, object keys sorted, array ordering retained, serde_json numeric representation. This is not RFC 8785. Decode untrusted bytes with `strict_decode` before typed validation to reject duplicate keys. Domain numeric invariants must be checked before canonical serialization; serde_json alone cannot validate a domain.

Rational wire values use decimal strings `{num,den}` to avoid JavaScript integer loss. They must be reduced, den positive, without leading zeros or negative zero. Rates use bounded positive u32 numerator/denominator. Frame/sample rounding is explicit and returns the exact rounding error. Cue anchors with unknown timings propagate UNKNOWN. No timestamp is invented. `Rate` has conversions to/from the existing video-domain FrameRate; musical time remains audio-specific.

`Owner.session` is taken from DriverExecutionContext/Broker, never caller args. HostSession denotes the existing host session principal binding, not a new identity service. `BaseStateSet` has a separate revision/fingerprint for every resource. Figma uses best_effort_revalidate; no native CAS claim.

PlanVault binds complete canonical plan bytes to a host owner. Hashes are lookup/integrity only. Reserve budgets before side effects. Failed or unknown attempts are never refunded or retried. Child repair plans inherit the root budget. Restart/expiry/revocation invalidates pending plans. Replan after fresh inspection. Caller must preserve ledger outcomes and pass all native actions through existing Broker policy.

A ValidationReport PASS needs every required versioned rule, deterministic exhaustive evidence on the same base and no failure. Fixture/simulation evidence does not certify native acceptance. Execution completion, evidence class and support level remain independent.

Audio review requested: silence/undefined loudness as typed unknown measurement; delayed feedback; partial effects; rates 44.1/48/96 kHz against rational video; weak revision semantics. Add consumer fixtures, not a forked Finding or clock. Changes are explicit C1/C2 revisions with migration tests.

C1 AV consumer note: a public final-audio receipt that will feed AV should preserve a path-free `MediaArtifact` and, when delivery is requested, populate the additive `ArtifactHandoffHint` with the artifact digest plus the provider-produced relative filename/path. Do not put a Broker root, absolute path or authority into the hint. The AV integration host binds that hint to the owner-configured readable root and uses the existing `artifact.handoff`; The audio subsystem does not need to call MLT or expose private Ardour/Faust helpers.
