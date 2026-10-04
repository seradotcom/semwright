# Build, install, start and uninstall

Want the shortest path? Start with the **[three-step quick start](quickstart.md)**. This page is the
complete reference for package selection, checksums, custom prefixes, source builds and removal.

Semwright is development software. R16 is CLOSED after separate revalidation. R06/R18 residual
physical/interactive cases remain OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT, not initial-v1
publication prerequisites. Independent security review and explicit maintainer authorization remain
required before public release. See [release policy](release-policy.md). There is no `curl | sh` installer.

## Install a candidate bundle

No public v1 release is created by this work. Obtain the native candidate from the maintainer's
exact-SHA **V1 multiplatform distribution** Actions run, together with its global manifest and
external `SHA256SUMS`. Match OS and architecture; do not select a package by filename alone.
Artifacts in this public repository are not confidential storage and expire after 30 days.

Each portable bundle contains all five commands (`semwright`, `semwrightd`, `semwright-mcp`,
`semwright-inspect`, `semwright-sandbox`), required packaged macOS native libraries where applicable,
licenses, install/support/security notes, an observe-only example, complete internal checksums and
install/uninstall helpers. Third-party applications, optional application drivers, models and user
data are not bundled or automatically installed. Native system dependencies still apply; a portable
archive is not a fully static operating-system image.

Verify the archive against its external checksum record before extraction, then inspect the included
helpers. The installer verifies every internal checksum before copying files. Hashes establish
matching bytes, not publisher identity or safety of an untrusted installer. Stop rather than bypass
an operating-system or organization security policy.

### Linux — x86_64 or aarch64

Substitute the actual package version and architecture:

```sh
sha256sum semwright-<version>-x86_64.tar.gz
# Compare with the matching entry in the external SHA256SUMS.
tar -xzf semwright-<version>-x86_64.tar.gz
cd semwright-<version>-x86_64
./install.sh
"$HOME/.local/share/semwright/bin/semwright" setup
```

The default prefix is `$HOME/.local/share/semwright`; an optional
`./install.sh --prefix "$HOME/path/to/new-semwright"` selects another new directory under HOME.
Do not use sudo for the portable installer. The archive targets the native Ubuntu 24.04 build
baseline; other Linux distributions/library versions require separate compatibility validation.

On compatible Debian/Ubuntu systems, the matching `.deb` is an alternative:

```sh
sudo apt install ./semwright_<version>_amd64.deb
semwright --help
# Later, to remove the package:
sudo apt remove semwright
```

Use `_arm64.deb` on aarch64. The package manager handles the system-wide `/usr/bin` installation;
it does not enable a service or give the daemon root authority. Never run the daemon as root.

### Windows — x86_64 or ARM64

Check the ZIP's SHA-256 against the matching external manifest entry, then extract it:

```powershell
Get-FileHash .\semwright-<version>-windows-x86_64.zip -Algorithm SHA256
Expand-Archive .\semwright-<version>-windows-x86_64.zip -DestinationPath .\candidate
Set-Location .\candidate\semwright-<version>-windows-x86_64
.\Install-Semwright.ps1
& "$env:LOCALAPPDATA\Semwright\bin\semwright.exe" setup
```

Use the `windows-arm64` ZIP for ARM64. Installation defaults to `%LOCALAPPDATA%\Semwright`,
requires no administrator rights, and accepts `-Prefix` for another new directory under your user
profile. It never edits PATH, registry, services or security settings. These are unsigned portable
ZIPs, not MSI/MSIX or Authenticode-signed applications. SmartScreen and script execution policy are
not bypassed. Where unsigned scripts are prohibited, stop and use an organization-approved
evaluation/distribution method. No reputation or signing claim is made.

### macOS — Apple Silicon or Intel

Use `macos-arm64` for Apple Silicon and `macos-x86_64` for Intel:

```sh
shasum -a 256 semwright-<version>-macos-arm64.tar.gz
# Compare with the matching entry in the external SHA256SUMS.
tar -xzf semwright-<version>-macos-arm64.tar.gz
cd semwright-<version>-macos-arm64
./install.sh
"$HOME/Library/Application Support/Semwright/bin/semwright" setup
```

The default prefix is `$HOME/Library/Application Support/Semwright`; `--prefix` selects another new
directory under HOME. Native runtime libraries remain beside the commands in the packaged layout.
These are unsigned/unnotarized archives, not notarized apps or TCC-certified deployments. Helpers do
not remove quarantine, disable Gatekeeper/SIP, change TCC databases or grant consent.

### Safe first-time setup

After any portable install, run the installed `semwright setup` command. It performs only local,
non-authorizing onboarding:

- creates the platform-native private config directory when missing;
- creates `daemon.toml` with the `observe` policy only when that file does not already exist;
- creates a ready-to-copy `mcp-client.json` snippet pointing at the exact sibling `semwright-mcp`;
- reports whether CLI, daemon, MCP and TUI binaries are present beside the running CLI;
- prints the broker, doctor and TUI next steps for the current installation.

The command is intentionally idempotent and **never overwrites** existing config/snippet files. Use
`semwright --dry-run --json setup` to inspect the proposed paths without writing anything.

It does not edit PATH, install/start a service, modify Claude/ChatGPT/Codex/other MCP clients, grant
broker policy, change TCC/portal settings, or approve sensitive operations. Those boundaries remain
explicit owner/OS actions. This keeps one simple setup command without turning onboarding into an
authority escalation.

### Removal and upgrades

Use the installed helper, or the extracted helper with the same explicit prefix:

```sh
# Linux default:
"$HOME/.local/share/semwright/uninstall.sh"
# macOS default:
"$HOME/Library/Application Support/Semwright/uninstall.sh"
```

```powershell
# Windows default:
& "$env:LOCALAPPDATA\Semwright\Uninstall-Semwright.ps1"
```

Stop any broker you started before uninstalling. Installation refuses every existing destination,
including an older install. The uninstaller validates all recorded files before deleting any;
changed, missing or redirected managed files cause refusal. It removes only unchanged receipt-owned
files, retains unknown files and never recursively deletes a user directory. Review a refused
removal rather than overriding it; normal upgrades use clean removal followed by a new installation.
External configuration, audit data, projects, application add-ons and consent remain untouched.

No helper changes PATH. Invoke installed absolute paths, or explicitly add the installed `bin`
directory to your own shell session. The installer prints the exact installed `semwright setup`
command, so first use does not depend on PATH. No daemon starts automatically. Setup creates the
private observe-only config when missing; an existing config is never replaced. Review
[permissions](permissions.md) before a live run and use an ordinary graphical-session user.
Installation/setup checks are not physical desktop or security certification.

## Prerequisites

The source quickstart targets **Ubuntu 24.04, x86_64**. Other platforms have separate
[host and evidence requirements](platforms.md). Use a disposable account, VM or hosted CI runner.
Compilation needs considerably more storage than a source checkout.

Install Git, Bash, a C/C++ build environment, pkg-config, Clang and PipeWire development headers.
On Ubuntu, an administrator can run:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y git build-essential pkg-config clang libpipewire-0.3-dev
```

Install Rust through its official distribution and make Cargo available in your shell.
`rust-toolchain.toml` pins **1.98.1**, including rustfmt and Clippy; the declared workspace MSRV is
**1.88**. Network access is needed for an empty dependency cache. Preserve `Cargo.lock` and use
`--locked`; do not regenerate it to get past a failed build. Never run the daemon as root.

## Development quickstart

Use a new directory, not a checkout containing work to preserve:

```sh
git clone https://github.com/seradotcom/semwright.git semwright
cd semwright
cargo build --locked -p semwright-daemon -p semwright-cli --bins
BIN_DIR=target/debug ./scripts/dev/fake-smoke.sh
```

This follows the development checkout, not a published release. Reproduce an evidence record by
checking out the source SHA named in that record, not assuming older evidence certifies current code.
The smoke creates a private runtime, starts `semwrightd --fake`, connects to its explicit fake socket,
runs doctor, discovers a control and executes `recipes/fake-export.yaml`. The typed recipe asserts
one exact Export match and returns the observed changed result; audit metadata is printed, and the
script cleans up its process/runtime. Its desktop policy is used only in the synthetic backend,
not recommended for a real desktop. This requests no real desktop, credentials, camera or microphone.

Inspect the effect and audit output, not just process exit. A fake smoke is not live application
or security certification. See [verification](../VERIFY.md).

Do **not** run `scripts/dev/bootstrap.sh` on an ordinary checkout. It is for initial dependency
resolution when `Cargo.lock` is absent and intentionally refuses the committed lock. Do not remove
the lock to make bootstrap pass.

## Optional full source build and legacy Linux user install

The quickstart builds two binaries. The source installer expects all five, so build in a suitable
environment with enough storage:

```sh
cargo build --locked --release \
  -p semwright-cli --bin semwright \
  -p semwright-daemon --bin semwrightd \
  -p semwright-mcp --bin semwright-mcp \
  -p semwright-tui --bin semwright-inspect \
  -p semwright-plugin-host --bin semwright-sandbox
python3 packaging/install/install.py --bin-dir target/release
```

This older Linux source installer writes to `$HOME/.local/bin`, refuses existing files, checks ELF
inputs and records hashes under `$HOME/.local/share/semwright-install`. It remains separate from the
new complete portable bundle prefix/receipt. It installs no service, model, application, policy or
portal grant. Add its binary directory to PATH explicitly. Do not mix the two removal mechanisms.

## Start a foreground broker

Before a live run, inspect the observe-only example and create a private configuration directory.
Copy the example to `$HOME/.config/semwright/daemon.toml` with mode `0600` only when that path does
not already exist. Do not overwrite another configuration. With your installed `bin` explicitly in
PATH, run as your normal user in the graphical login session:

```sh
semwrightd --config "$HOME/.config/semwright/daemon.toml"
# In a second terminal in the same login session:
semwright --json doctor
semwright ui snapshot --max-nodes 100
```

Observe access can reveal application labels. Sensitive actions need a separate foreground
`--approval-console`; do not connect an agent to the operator terminal. See [permissions](permissions.md).

## Optional user service

Only after the foreground path succeeds, review `packaging/systemd-user/semwright.service` in the
source repository. Its sample ExecStart targets the legacy `~/.local/bin`; for a portable install,
explicitly adjust it to that bundle's installed `bin/semwrightd` path. Install the unit only in your
own systemd-user configuration without replacing another unit, reload the user manager and explicitly
enable it. It is never a root service and has no approval console, so sensitive requests return
`ConsentRequired`. Stop it before starting another broker on the same endpoint. No service or
lingering setting is installed by default.

## Distribution and verification

The native package set is Linux x86_64/aarch64 tar.gz and amd64/arm64 deb, Windows x86_64/ARM64 ZIP,
and macOS arm64/x86_64 tar.gz: **six native jobs and eight package files**. Linux binaries are never
used as Windows/macOS payloads. Architecture, internal checksums, deterministic package assembly,
extraction, shipped-helper installation, `semwright setup` dry-run/creation/idempotence, installed
smoke, removal, cleanup, overwrite refusal and changed/unowned-file preservation are checked per
native job. Linux also exercises the actual deb package-manager lifecycle in disposable hosted CI.

`V1_DISTRIBUTION_MANIFEST.json` verifies the actual package hashes against all six exact-SHA native
certificates. Its `release_admission=false` is intentional. Reproducibility here covers repeated
assembly from the same binaries, not independent reproducibility of every compiler input/output.
The macOS/Windows sandbox command remains its documented fail-closed compatibility sentinel;
including five commands is not a claim of identical cross-platform isolation functionality.

See [release policy](release-policy.md), [platforms](platforms.md) and the exact run/SHA record.
Green packaging does not imply physical Hyprland, unlocked-Windows, TCC, signing or independent
security acceptance. A tag cannot bypass public admission and an Actions artifact is not a release.

## Remove a legacy source-built Linux install

Stop and disable a user service you explicitly installed, remove only that unit and reload the
user manager. Then use the source installer's own manifest:

```sh
python3 packaging/install/uninstall.py
```

Only unchanged recorded binaries are removed. Changed binaries cause refusal. Configuration, audit,
projects, browser leftovers, portal grants and application add-ons remain for explicit review.
Do not delete an active socket to force shutdown.
