# Native SDK security delta

This document describes the Native SDK-specific security boundary and conformance expectations.

## Authority boundaries

- Broker/Policy remains the only authorization/approval authority; Native descriptors do not grant permission.
- Driver Host owns provider/session identity, native-ref resolution, filesystem/tool grants, cancellation, sandbox lifecycle and descendant cleanup.
- The application owns model, native revision, transaction, deduplication receipts and domain effects.
- Project Graph owns CURRENT/STALE/UNKNOWN and provenance; the Native Graph adapter can only build untrusted candidates/locators.
- Effect Conformance owns findings/verdicts; the SDK cannot mint PASS.

## New/changed attack surfaces

- Opaque revision and generation mapping into ephemeral Native refs.
- App-owned CAS versus preflight-only validation.
- Historical lookup, request-key reuse, retention expiry and unknown outcomes.
- Revision-bound pagination and bounded event resynchronization.
- Host-mediated TypeScript process bridge, exact bundle SHA-256 and owner-granted mounts/runtime tools.
- Publication candidate/destination/request binding; `PublicationProvider` is routed through the same explicit operation contract and app transaction.
- Artifact handoff/admission and protected readback used by Effects and Graph.
- Binding parity for JSON bounds, exact integers and canonical request digest.

## Fail-closed properties

Unknown protocol/descriptor drift is rejected. Direct authority-bearing `NativeDriver::execute` is denied. An operation cannot select an ambient executable, module, source path or new mount. Old session refs fail after rebind. Missing historical receipts do not authorize replay. A malformed/lost post-mutation reply remains uncertain. A wrong artifact/candidate digest is rejected. An application cannot mount the protected admitted/readback destination.

## Conformance requirements

Real Host acceptance runs only in private disposable Actions with the repository `semwright-sandbox`; the workflow sets the same sandbox helper/driver-sandbox gates used by canonical security tests. Zero-test selectors, skips and sandbox relaxation are failures. CircleCI may iterate compatible library/binding lanes but does not replace final Actions closure.
