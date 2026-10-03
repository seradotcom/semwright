# Build, install, start and uninstall

Semwright is development software. This guide starts with a synthetic desktop; it does
not request real desktop, microphone, camera, clipboard or browser-profile access.
Independent R16 review and remaining physical/interactive release gates are open.
There is no verified release-download path in this guide and no `curl | sh` installer.

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

## Pinned source quickstart

Use a new directory, not an existing checkout with work you want to keep:

```sh
git clone https://github.com/seradotcom/semwright.git semwright
cd semwright
git checkout --detach 6491c0d838fa066938a494524d69ed507aa0dbe8
```

This is the source frozen for the R16 documentation review, not a release approval. Its
engineering ancestor and platform evidence retain their own SHAs. From that checkout:

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

The repository contains tar/deb packaging, a Nix build expression, SBOM generation and
scoped attestation workflows. Historical hosted certification is recorded in
[release blockers](../RELEASE_BLOCKERS.md) and [verification](../VERIFY.md). Consult the
source/run record rather than assuming every one of these gates ran on a new commit.
The quickstart above is not a new Nix build or package-install certification.

For future downloaded artifacts, establish an independently trusted source of checksums
and publisher provenance before extraction. `sha256sum -c SHA256SUMS` verifies matching
bytes, not publisher identity. A checksum next to an untrusted archive is not a signature.
No release is published by these instructions; release admission remains fail-closed.

## Uninstall

Stop and disable a user service you explicitly installed, remove only that unit, and reload
the user manager. Then use the installer manifest:

```sh
python3 packaging/install/uninstall.py
```

Only unchanged files matching the manifest are removed. Changed binaries cause a refusal.
Config, audit, project data, browser leftovers, portal grants and application add-ons are
retained for explicit review/removal. Do not delete an active socket to force shutdown.
