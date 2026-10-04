# ADR 0001: Host-mediated process bridge

**Status:** accepted.

## Context

A TypeScript application must retain its own runtime/model/persistence without letting operation JSON choose an executable or bypass Driver Host. An in-process provider cannot demonstrate the real sandbox/permission boundary.

## Decision

The Rust adapter requests only an owner-pinned sealed runtime tool through `DriverExecutionContext`. Installation configuration fixes tool name, bundle mount/file/SHA-256, data/output mounts and timeout. Driver Host materializes grants and owns process/sandbox lifecycle. The bridge exchanges one bounded JSON frame on stdin/stdout.

No caller field can select source code, executable path, shell command, environment secret, mount or Broker approval. The bundle is re-hashed before execution.

## Consequences

- TypeScript owns SQLite transactions and recovery independently of Semwright storage.
- Runtime failure after mutation can remain uncertain and is not blindly retried.
- The current materialized Node profile is Linux-specific and does not imply Windows/macOS Host certification.
- Direct/in-process app tests are useful local contract tests but cannot be cited as real Native Host acceptance.
