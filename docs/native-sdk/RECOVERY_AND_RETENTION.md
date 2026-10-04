# Recovery and retention

`RequestIdentity` binds resource, explicit epoch, request key and exact request digest. Reusing a key for different request content is a conflict, not a safe replay.

Recovery states are explicit: `Recorded`, `Pending`, `OutcomeUnknown`, and `RetentionExpired`. Lookup is historical only. Current access must be checked first, and a historical receipt cannot authorize or replay an operation.

## Uncertain completion

If an effect may have committed but a reply is lost or malformed, outcome remains uncertain until application evidence reconciles it. Retrying under a new key is not a generic recovery mechanism. The SQLite reference consumer proves lost-response recovery after reopen without applying the effect twice.

## Sustainable retention

The reference consumer rotates receipt epochs and keeps bounded receipt/event windows. A request in an expired epoch never becomes a fresh safe key. Conformance performs 1300 durable operations, verifies bounded retained receipts/events and refuses expired-key reuse.

Retention is an application/profile property. The base SDK does not introduce a hidden second journal.
