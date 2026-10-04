# Exact-SHA validation plan

No Cargo compilation, Rust tests, application runtimes or TypeScript compilation
run on the workstation. GitHub Actions is final execution evidence; CircleCI is
iteration evidence only. Jobs use shared scripts and separate branch filters.
Existing workflows and CircleCI A-side jobs are preserved.

Initial public lane: source-lock and evidence-classification tests only.
Private source lanes: portable contracts/MSRV, file-backed regressions,
TypeScript bridge/external consumers and real Driver Host/daemon integration.
Derived sources and outputs must not be uploaded by a public workflow while
license review is unresolved. A private validation repository is temporary CI
storage, not a new product/backend or canonical upstream.

Every report binds exact HEAD, tree, Cargo.lock digest, suite script digest,
CI provider, OS and profile. Test count must be positive. A skipped or unsupported
profile remains NOT_RUN/BLOCKED, never PASS. Dependency-resolution diagnostics
are distinct from a later --locked run on a committed lockfile. Read failed logs,
fix the cause and rerun against the new immutable SHA.

Final integration must demonstrate CLI and MCP through daemon, Broker/Policy,
real Driver Host, native persistence, canonical artifact admission, protected
readback, Graph and Effects. No in-process provider or direct-stdio substitute.
