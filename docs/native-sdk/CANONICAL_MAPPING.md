# Authority mapping

| Concern | Existing owner | Native SDK boundary |
|---|---|---|
| Risk, idempotency, command schema | semwright-types | Reuse CommandDescriptor; no uniform reversible default |
| Wire protocol and native references | semwright-driver-sdk | Implement Driver; negotiate required interfaces |
| Permissions, consent, mounts | Broker/Policy and Driver Host | Context supplied by Host, never caller JSON |
| Model, storage, native transactions | Application | SDK does not serialize every app into a document |
| Jobs/progress/artifacts | Canonical Driver/Host job interfaces | Optional adapter; no scheduler |
| Revisions and dependencies | App observation + Project Graph | Submit candidates; trusted adapter admits evidence |
| Effects, prepared plans, reports | Composition and effect-conformance | Reuse types/evaluator; no second authority |
| Driver distribution | driver-registry | Existing package and installation contracts |
| Platform | Platform service/client | Separate optional consumer, not SDK definition |

Opaque app revisions must be compared in full within the application's commit
transaction. A native target fingerprint is not permission and must not replace
that transactional CAS. Observations and receipts must retain source generation,
request digest and evidence coverage. Missing records remain UNKNOWN.

Launchwright is an external consumer. Its domain types, accounts and production
workflow must not become required Native SDK interfaces.
