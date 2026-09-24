# Windows packaging

First supported delivery should be a per-user portable ZIP or per-user installer that places Semwright under a user-owned application directory and starts it in the interactive user session. Administrator rights are not required for normal usage. A privileged Windows Service is explicitly not the default architecture.

MSIX/MSI can be evaluated as distribution formats, but MSIX packaging is not a replacement for Driver Host isolation. Release signing should use Authenticode in protected release workflows; PR CI must never require signing secrets. SmartScreen/reputation effects should be documented rather than bypassed. `winget` is a future distribution channel after a signed stable package exists.

Native deliverables should be produced separately for AMD64 and ARM64. At install/startup the PE verifier rejects architecture mismatch. No `uiAccess` manifest is requested.
