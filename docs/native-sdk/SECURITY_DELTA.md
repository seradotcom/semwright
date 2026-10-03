# Native SDK security delta

This is an implementation review, not an independent R16 certification.

No Broker grant, consent, sandbox flag or launch policy is changed by N0.
No received script or binary was executed during inventory. Original private
inputs are retained outside public tracked source. No license was assigned.

New boundaries to test: current authorization before historical lookup;
uncertain mutation outcomes and deduplication beyond retention; opaque revision
CAS inside app transactions; observation cursor scope/generation; native ref
revocation after reopen; artifact producer/admission binding; private candidate
publication CAS; TypeScript limits matching Rust; Host cleanup and cancellation.

Synthetic owned fixtures only. Missing platform support must fail closed, not
activate trusted-native execution or relax sandbox settings. Protected expected
values must stay outside the application-readable workspace.
