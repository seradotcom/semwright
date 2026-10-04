# Native SDK TypeScript binding

This binding exposes bounded cooperation values, opaque revisions, observation
pages and request-bound recovery. It does not own an application model, database,
transaction, Broker permission, canonical evidence admission or job scheduler.

The inventory example uses its own SQLite schema and native transactions.
`applicationContext` is an application-local context, never a Broker credential.
`bridgeEntrypoint` is a data dispatcher used inside an owner-installed process
profile. Driver Host remains the runtime and permissions boundary.

A materialized Node bridge is currently an explicit Linux profile. No Windows or
macOS Host execution is inferred from portable TypeScript tests.

Builds and tests run in CI. The package follows the repository license: MIT OR Apache-2.0.
