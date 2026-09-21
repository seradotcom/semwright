# Continue this repository, do not regenerate it

Read README.md, VERIFY.md, RELEASE_BLOCKERS.md and ACCEPTANCE.md, then the original
requirements under docs/requirements. The actual source exists throughout the workspace;
this is not a request for another architectural proposal.

Priority is the first real Rust build. Install/use a toolchain on the build machine,
generate and review Cargo.lock, pin the actual compiler version, run fmt/check/tests,
fix concrete type/API/ownership failures, then Clippy/docs. Do not report the Python,
JavaScript, C or Python-CDP test results as Rust tests. Preserve the exact distinction
between authored, syntax-checked, compiled, unit-tested and live-verified code.

Then run core fake integration and fake-smoke, inspect the central policy/ref/audit gates,
exercise a real MCP client, test plugin sandbox negative cases, and validate each native
backend/application in an isolated account. Close the explicit implementation gaps rather
than returning fake data or a generic success. X11 I/O/lifecycle, output schemas, portal
EIS/PipeWire/persistence, Chromium quotas/crash cleanup, plugin handshake conformance and
recipe progress/taint need substantive work.

Source tests and scripts are included, but build/CI/fuzz/coverage/benchmark/package results
are not invented. Run the acceptance checklist again and attach exact logs. Keep all
release-readiness flags false until their evidence really exists. Do not redesign away
failed tests, broaden privileges, touch unrelated user processes, or start unbounded
background fuzzers. Deliver a clean archive and explicit remaining limitations.
