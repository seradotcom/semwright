# Frozen Semwright snapshot

BASELINE_SHA: `a14abd8328e092a8227584750e47c38a77449ffa`

Fetched once with `git fetch origin --prune` on 2026-09-23. No moving-main dependency.
Branch: `feat/motion-canvas-driver`. The canonical checkout is not an implementation workspace.

The snapshot includes the CLI rename (PR 37), OBS (PR 32), post-platformization
AT-SPI (PR 36), EIS (PR 31), the macOS host foundation, Universal Linux runtime,
Driver Host/SDK, Registry/Distribution, Provider Runtime, MCP federation,
Chromium, Blender, LibreOffice, KiCad and MLT video.

All five principal workflows for this exact SHA were completed/success:

| Workflow | Run |
|---|---|
| Quality gates | 35825725023 |
| Dependency, coverage and fuzz gates | 35825725028 |
| Native application integration | 35825724990 |
| Platformization and macOS | 35825725019 |
| OBS driver integration | 35825725044 |

Driver Manifest and Protocol are both version 1, transport `stdio_v1`.
The DriverProvider attests the executable and descriptor digests, mounts explicit
owner filesystem grants at `/workspace/<grant>`, and enforces Bubblewrap plus
Landlock on Linux. Existing system tooling exposure is inherited, not expanded
by the Motion Canvas driver. Registry packages pin an executable and its manifest;
they do not currently distribute a complete Node/browser runtime.

Protocol v1 does not negotiate child events, dynamic capabilities or cooperative
cancellation. Rendering uses driver-local jobs with explicit status/cancel/result
capabilities; this must not be described as broker-negotiated job progress.

The existing host's virtual-address-space ceiling is 4 GiB. Browser compatibility
with that ceiling must be measured before changing any generic contract.
Windows is not certified by this baseline. No Windows launch claim is permitted.

Resource policy for this mission: builds, tests, fuzzing and renders run in GitHub
Actions. Do not create a local `target` or local browser/runtime installation on
the owner's machine. Final media belongs in Actions artifacts, not Git history.
