# Bindings

## TypeScript

`sdk/native-typescript` is an executable binding, not schema-only documentation. It implements the shared bounded JSON boundary, opaque revisions, revision-bound pagination, request/recovery validation, exact application request digests, local non-authoritative contexts and bridge framing.

`contracts/native/cooperation-vectors.json` is consumed by both Rust and TypeScript tests so digest and opaque-revision semantics cannot drift independently.

## Host-mediated Node bridge

The optional Rust `process-bridge` executes an owner-pinned JavaScript bundle only through a Driver Host sealed runtime tool. Installation configuration fixes tool, bundle mount/file/SHA-256, data/output mounts and timeout. Operation JSON cannot choose an executable, module, source path, shell command, mount or Broker approval.

The exact bundle is re-read and hashed per invocation. Runtime paths come from Host grants. The application receives invocation facts, not new authority.

The materialized Node bridge is currently an explicit Linux Host profile. Portable TypeScript tests on Windows/macOS do not imply native Node Host execution there.

C ABI, Python, Swift/Kotlin, browser WASM and remote SaaS transports remain extension points and are not implemented by this SDK.
