# Build, install, start and uninstall

Semwright is development software. This guide starts with a synthetic desktop; it does
not request real desktop, microphone, camera, clipboard or browser-profile access.
R16 is closed after separate revalidation; R06/R18 physical/interactive certification remains
`OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT`. Release admission is still fail-closed.
There is no `curl | sh` installer.

## Prerequisites

The bounded quickstart targets **Ubuntu 24.04, x86_64**. Other platforms have separate
[host and evidence requirements](platforms.md). Use a disposable account, VM or hosted CI
runner. Compilation needs considerably more storage than a source checkout.

Install Git, Bash, a C/C++ build environment, pkg-config, Clang and PipeWire development
headers through the operating system's package manager. On Ubuntu, an administrator can run:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y git build-essential pkg-config clang libpipewire-0.3-dev
```

Install Rust through its official distribution and make Cargo available in your shell.
`rust-toolchain.toml` pins **1.98.1**, including rustfmt and Clippy; the separately declared
workspace MSRV is **1.88**. Network access is needed for an empty dependency cache. Review
`Cargo.lock`; do not regenerate it to get past a failed build. Never run the daemon as root.

## Development quickstart

Use a new directory, not an existing checkout with work you want to keep:

```sh
git clone https://github.com/seradotcom/semwright.git semwright
cd semwright
```

This follows the current development checkout; it is not a published release. Reproducible
verification records name the exact source SHA they exercised. If you are reproducing one of
those records, check out the SHA from that record rather than treating an older R16 snapshot as
the normal first-use version. See [verification](../VERIFY.md).

From the checkout you intend to evaluate:

```sh
cargo build --locked -p semwright-daemon -p semwright-cli --bins
BIN_DIR=target/debug ./scripts/dev/fake-smoke.sh
```

The build selects the daemon and CLI needed by the example rather than every native
application. The smoke creates a private temporary runtime, starts `semwrightd --fake`,
connects explicitly to its fake socket, runs `doctor`, discovers a control and executes
`recipes/fake-export.yaml`. That recipe asserts one exact Export match and returns the
invocation's `changed` result. Metadata audit is printed; the process and temporary
directory created by the script are cleaned up on exit. It uses the desktop policy only
inside that synthetic backend, not as a recommended policy for a real desktop.

A successful process exit is not proof of every intended effect. Inspect the recipe and
audit output. R16 smoke evidence records the observed result and names the tested source
and suite separately. See [verification](../VERIFY.md).

Do **not** run `scripts/dev/bootstrap.sh` here. It is for initial dependency resolution
when `Cargo.lock` is absent and intentionally exits with an error when the committed lock
exists. Do not remove the lock to make the bootstrap pass.

## Optional full build and local user install

The quickstart builds only two binaries. The Linux installer expects all five packaged
executables, so prepare the complete release build in an environment with enough space:

```sh
cargo build --locked --workspace --release
python3 packaging/install/install.py --bin-dir target/release
```

The installer writes to `$HOME/.local/bin`, refuses existing files, checks the ELF inputs
and records hashes under `$HOME/.local/share/semwright-install`. It installs no model,
application runtime, service, policy or portal grant. ELF validation and a checksum do
not establish publisher identity. Add the binary directory to your PATH explicitly.

Before a live run, inspect `config/observe.toml` and create a private configuration directory.
Copy it to `$HOME/.config/semwright/daemon.toml` with mode `0600` only when that path does
not already exist. Do not overwrite another configuration. Start a foreground broker
as your normal user in the graphical login session:

```sh
semwrightd --config "$HOME/.config/semwright/daemon.toml"
# In a second terminal in the same login session:
semwright --json doctor
semwright ui snapshot --max-nodes 100
```

Observe access can reveal application labels. Sensitive actions need a separate foreground
`--approval-console`; do not connect an agent to the operator terminal. See [permissions](permissions.md).

## Optional user service

Only after the foreground path succeeds, review `packaging/systemd-user/semwright.service`.
Install it into your own systemd user configuration without replacing another unit, reload
the user manager and explicitly enable it. This is never a system-wide root service.
The unit has no operator console, so sensitive requests return `ConsentRequired`. Stop it
before starting another broker on the same endpoint. No lingering service is installed by default.

## Distribution and verification

The v1 distribution gate builds and certifies native packages separately; Linux artifacts are not
reused as Windows/macOS payloads. The expected package set is:

- Linux: `semwright-<version>-x86_64.tar.gz`, `semwright-<version>-aarch64.tar.gz`,
  `semwright_<version>_amd64.deb`, and `semwright_<version>_arm64.deb`.
- Windows: `semwright-<version>-windows-x86_64.zip` and
  `semwright-<version>-windows-arm64.zip` containing native PE executables.
- macOS: `semwright-<version>-macos-arm64.tar.gz` and
  `semwright-<version>-macos-x86_64.tar.gz` containing native Mach-O executables.

Every portable archive contains the five command binaries, README/install notes, dual licenses,
SECURITY/SUPPORT scope, platform notes and an internal `SHA256SUMS`. CI also emits an external
manifest for each package plus a combined `V1_DISTRIBUTION_MANIFEST.json` after all six native jobs
pass. Linux additionally certifies per-user install/uninstall and `.deb` extraction.

Windows packages are portable ZIPs, not MSI/MSIX, and are not Authenticode-signed; SmartScreen or
reputation warnings remain possible. Remove the extracted directory to remove the portable copy.
macOS packages are unsigned/unnotarized CLI archives; they are not equivalent to a notarized app or
TCC-certified installation. Codesign/notarization/SmartScreen reputation are post-v1 distribution
hardening unless signing infrastructure is explicitly provisioned.

Historical hosted certification is recorded in [release blockers](../RELEASE_BLOCKERS.md) and
[verification](../VERIFY.md). Consult the source/run record rather than assuming every gate ran on a
new commit. `sha256sum -c SHA256SUMS` verifies matching bytes, not publisher identity. GitHub Release
asset publication remains behind `scripts/release/assert-ready.py`; a green package job alone does
not authorize a release.

## Uninstall

Stop and disable a user service you explicitly installed, remove only that unit, and reload
the user manager. Then use the installer manifest:

```sh
python3 packaging/install/uninstall.py
```

Only unchanged files matching the manifest are removed. Changed binaries cause a refusal.
Config, audit, project data, browser leftovers, portal grants and application add-ons are
retained for explicit review/removal. Do not delete an active socket to force shutdown.
