# Jobs and recovery

Use `jobs.start`, `jobs.get`, `jobs.list`, and `jobs.cancel` only for capabilities that participate in Semwright's job contract.

A successful dispatch is not the same as a completed job. Read terminal state and artifacts from the job contract. Cancellation is a request; after cancellation or transport failure, inspect state before deciding whether another mutation is safe.

For ordinary errors:

- schema/invalid argument: describe again and correct the typed input;
- stale ref: inspect again;
- unavailable route: do not reinterpret it as a permission problem;
- policy/permission denial: do not route around the Broker;
- timeout or unknown outcome: verify state before retrying;
- known no-effect backend failure: choose a documented recovery path, not an automatic different backend.
