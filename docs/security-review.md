# Independent security review packet

This packet turns release blocker R16 into a reproducible third-party review task.
It does **not** certify Semwright and must not be used as self-attestation.

## Baseline

The reviewer must record one immutable Git commit before starting. All findings,
commands, artifacts and conclusions refer to that commit. Do not silently carry
evidence from another SHA into the review.

Record at minimum:

- commit SHA and repository URL;
- reviewer name or organization and review dates;
- host/kernel/distribution used for Linux sandbox tests;
- Rust toolchains and security-tool versions;
- which live desktop/application tests were actually executed.

### Reviewer handoff procedure

The maintainer may prepare an immutable source handoff, but that handoff is **UNREVIEWED** and is
not closure evidence by itself. Use the exact commit, not the maintainer working tree:

```sh
BASELINE_SHA=$(git rev-parse HEAD)
SHORT_SHA=${BASELINE_SHA:0:12}
git archive --format=tar --prefix="semwright-${SHORT_SHA}/" "$BASELINE_SHA" \
  | gzip -n -9 > "semwright-security-review-${SHORT_SHA}.tar.gz"
sha256sum "semwright-security-review-${SHORT_SHA}.tar.gz" \
  > "semwright-security-review-${SHORT_SHA}.sha256"
printf '%s\n' "$BASELINE_SHA" > "semwright-security-review-${SHORT_SHA}.baseline"
```

The reviewer should independently verify the archive hash, record the baseline SHA in the final
report, and work from a fresh extraction or clone. The report must identify every required review
area as executed, not executed or blocked; list exact tool versions and commands; and give each
finding an explicit remediation status. A maintainer-generated archive, green CI, zero automated
findings or a completed checklist cannot substitute for the independent reviewer conclusion.

## Trust model to challenge

Semwright treats the broker and owner policy as trusted. Agent intent, MCP clients,
observed application/page text, driver/plugin payloads and external MCP metadata are
untrusted. A same-UID malicious host process and compromised kernel are outside the
claimed isolation boundary. App-native drivers invoke applications that retain their
ordinary user privileges.

The review must test whether untrusted inputs can cross a boundary that policy did not
grant, not whether an LLM can be convinced to ask for a dangerous operation.

## Required review areas

1. **Authorization and confirmation** — profile grants, explicit allow/deny precedence,
   application/path scoping, privilege-sensitive approval, and inability of a requesting
   client to approve its own operation.
2. **IPC and session identity** — socket ownership/mode, peer UID, session tickets,
   request bounds, cancellation ownership and cross-session isolation.
3. **Object identity and focus** — stale refs, generation/fingerprint reuse, focus drift,
   pre-dispatch validation and no input fallback after a failed focus precondition.
4. **Portal authority** — native user consent, revocation, cancellation, restore-token
   confidentiality, EIS lifecycle, clipboard and ScreenCast authority separation.
5. **Filesystem confinement** — openat2/FD-relative traversal, symlink/hardlink/mount
   escape, atomic writes, root replacement/TOCTOU assumptions and sensitive-root denial.
6. **Plugin/driver isolation** — digest/descriptor attestation, Bubblewrap, Landlock,
   environment scrubbing, mount rights, network opt-in, RLIMITs, watchdog and descendants.
7. **Federated MCP** — hostile schemas/descriptions/results, crash/reconnect generations,
   secret forwarding, namespacing and proof that federation re-enters broker policy.

8. **Prompt-injection containment** — application/page text must remain data; verify that
   labels, errors, external metadata and artifact names cannot grant authority or smuggle
   reserved provenance fields.
9. **Audit and disclosure** — no sensitive bodies in audit, bounded rotation, error
   redaction, terminal escaping, private/expiring artifacts and honest uncertain outcomes.
10. **Resource and lifecycle safety** — frame/output limits, timeouts, cancellation races,
    partial effects, retry semantics, event backpressure and crash cleanup.
11. **Supply chain** — lockfile, audit/deny, pinned Actions, reproducible packages/SBOMs,
    provenance attestations and fail-closed release admission.
12. **Platform boundaries** — Linux enforcement must not be weakened by portable abstractions;
    macOS/Windows claims must match executed evidence and their native consent/security models.

## Minimum reproducible commands

Run these from the recorded commit, adding the native development libraries documented
by CI for the review host:

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features --doc
cargo audit --deny warnings
cargo deny --locked check
python3 -m unittest discover -s tests/python -v
node --test tests/js/bridge.test.mjs
```

The reviewer should also execute the dedicated hostile-process suites on Linux. The
plugin/driver hostile tests are feature-gated and ignored by default, so reproduce the
same explicit opt-in used by hosted CI:

```sh
cargo build --locked -p semwright-plugin-host --features test-tools \
  --bin semwright-sandbox --bin semwright-adversarial-plugin-fixture
SEMWRIGHT_TEST_PLUGIN_SANDBOX=1 \
  cargo test --locked -p semwright-plugin-host --features test-tools \
  --test adversarial -- --ignored --nocapture

cargo build --locked -p semwright-driver-host --features test-tools \
  --bin semwright-adversarial-driver-fixture
SEMWRIGHT_TEST_DRIVER_SANDBOX=1 \
  SEMWRIGHT_TEST_SANDBOX_HELPER="$PWD/target/debug/semwright-sandbox" \
  cargo test --locked -p semwright-driver-host --features test-tools \
  --test adversarial_sandbox -- --ignored --nocapture

cargo test --locked -p semwright-core --test broker_contract -- --nocapture
cargo test --locked -p semwright-core --test provider_runtime -- --nocapture
cargo test --locked -p semwright-platform-linux --test eis_transport -- --nocapture
```

A hostile-suite invocation that reports `running 0 tests` is not review evidence. Treat it
as a harness/configuration failure and correct the feature/ignored-test invocation before
continuing.

Hosted CI evidence is useful but not a substitute for reviewing the code paths that make
the test meaningful. For fuzzing, inspect the exact pinned nightly/cargo-fuzz versions and
the target list in `.github/workflows/security.yml`; record corpus duration and crashes.

## Adversarial cases that must be attempted

At minimum attempt: traversal and symlink swaps; stale-reference reuse; focus change between
observation and input; cancellation before and after dispatch; oversized/malformed frames;
duplicate JSON keys; malicious driver descriptors; hostile MCP metadata/results; output and
event flooding; child-process escape/cleanup; environment-secret discovery; loopback/network
escape; audit failure near a side effect; terminal-control injection; and portal revocation
while a request is pending.

Use disposable fixtures and accounts owned or explicitly authorized by the reviewer. Never
place real credentials in a proof of concept or public issue.

## Known non-claims to preserve

A review must not silently upgrade these into guarantees:

- same-UID hostile processes are outside the local IPC threat boundary;
- Bubblewrap/Landlock regressions are not a formal kernel-isolation proof;
- recipe taint/redaction is not formal information-flow noninterference;
- app-native adapters do not sandbox the target application itself;
- browser origin restrictions are not a network firewall;
- macOS TCC and Windows consent/isolation are not equivalent to Linux sandboxing;
- remote marketplace publisher identity is not implied by local SHA-256 package checks;
- live support claims require the exact desktop/application evidence documented in VERIFY.md.

## Finding format

Each finding should contain: ID, severity, affected commit/path, violated boundary, minimal
reproduction, expected vs actual behavior, impact, whether an effect may already have
occurred, proposed remediation and regression-test recommendation. Secrets and unrelated
user data must be redacted.

## R16 closure evidence

R16 may be marked closed only after an independent reviewer supplies a dated report tied to
the reviewed commit, covers every required area above, and identifies any unresolved
release-blocking findings. The maintainer then records the report reference and remediation
SHAs in RELEASE_BLOCKERS.md/VERIFY.md. Absence of findings from automated tools alone is not
an independent security review.
