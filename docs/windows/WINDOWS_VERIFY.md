# Verification commands — intentionally not executed by authoring session

Run only after applying this drop to the exact baseline or a reviewed rebased equivalent.

1. Confirm `git rev-parse HEAD` matches the expected integration base before applying replacements.
2. Run existing Linux/macOS workflows unchanged.
3. Run `.github/workflows/windows-platform.yml` on GitHub-hosted native x64 and ARM64 runners.
4. On Windows, then run the repository equivalents of `cargo fmt --all -- --check`, workspace check/clippy/test/doc, cargo audit, cargo deny, coverage and bounded fuzz/property suites.
5. Build `fixtures/windows-uia` and run the live matrix on an unlocked disposable Windows 11 session.

Do not label a hosted CI run `PASS_WINDOWS_INTERACTIVE`. W4 requires native CI evidence; W5 requires a real interactive desktop. Driver Host remains fail-closed until secure-spawn is implemented, even if all compile checks are green.

## Screen capture closeout

Hosted Windows CI may certify compilation and the bounded D3D11/PNG/artifact helpers, but it
must not be relabelled as interactive picker evidence. W5 additionally requires an unlocked
Windows 11 desktop to exercise screen.capture through the system GraphicsCapturePicker,
including user cancellation, target close/resize, timeout, artifact expiry and confirmation
that no image bytes enter audit logs. Programmatic CreateForWindow remains an internal
revalidated primitive and is not a substitute for the public command's consent flow.
