# Build, install, start and uninstall

## Before installation

No release binary is published yet. The accepted development baseline has a committed
Cargo.lock, a pinned Rust 1.98.1 toolchain and green hosted build/test gates, but Semwright
is still development software with live-desktop and security evidence outstanding. Start
with the developer instructions in a disposable account/VM before granting access to real
work. There is no verified release URL and no `curl | sh` installer.

```sh
./scripts/dev/bootstrap.sh
cargo fmt --all
cargo check --locked --workspace --all-targets
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
BIN_DIR=target/release ./scripts/dev/fake-smoke.sh
```

The bootstrap can generate/resolve a lockfile only when absent; the repository currently
commits the reviewed development lockfile. It does not install Rust, run sudo or launch
desktop automation. Review dependency licenses/advisories and run all
[gates](../scripts/ci/rust-gates.sh) before treating the build as a release.

## Local user install

After a successful reviewed build:

```sh
python3 packaging/install/install.py --bin-dir target/release
```

This writes exactly five executables to `$HOME/.local/bin`, refusing to overwrite existing
files, and records hashes under `$HOME/.local/share/semwright-install`. It checks ELF input
and ownership/permissions; it does not prove a source build's correctness or authenticity.
It installs no model, browser, toolchain, portal permission, configuration or service.
Add `$HOME/.local/bin` to your own PATH when necessary.

Use `config/observe.toml` as the initial daemon config, copied to
`$HOME/.config/semwright/daemon.toml` with mode `0600`. Keep the directory private. Do not
replace an existing file without reviewing it. Start a foreground daemon to inspect
startup failures and `doctor`; start with observe-only capabilities.

Optional federated MCP server definitions live separately at
`$HOME/.config/semwright/mcp-upstreams.toml` (or the XDG equivalent) and are managed by
`computerctl mcp upstream ...`. Adding or enabling a definition never edits daemon policy;
an `external-mcp:<slug>` grant must be reviewed separately. Registry changes currently take
effect after restarting the broker. See [MCP federation](mcp-federation.md).

## Optional systemd user service

Only after the foreground path succeeds:

```sh
mkdir -p "$HOME/.config/systemd/user"
install -m 644 packaging/systemd-user/semwright.service "$HOME/.config/systemd/user/semwright.service"
systemctl --user daemon-reload
systemctl --user enable --now semwright.service
journalctl --user -u semwright.service --since '5 minutes ago'
```

The unit expects binaries and config at the default paths above. It has no operator
console. Sensitive actions therefore fail with `ConsentRequired`; do not weaken risk
classes to make an unattended service approve them. Stop the unit before starting a
separate foreground broker with `--approval-console`. Never use `systemctl` system-wide
or sudo for this core. No lingering login service is configured by the installer.

## Distribution packages

`scripts/release/package.py` configures x86_64/aarch64 ELF validation, a tarball, checksums,
and optional `.deb` via dpkg-deb. Release admission is deliberately blocked by
`release-readiness.json`. No tarball containing binaries or Debian package was built here.
The Debian path has no auto-enable maintainer scripts. Its runtime dependencies/ABI must
be validated on intended distro versions before distribution. RPM is not provided.

`packaging/nix/package.nix` is a guarded buildRustPackage expression. The repository now
has a reviewed development `Cargo.lock`, but the Nix expression itself has not been evaluated;
no `flake.lock` or Nix build is claimed. Release workflows select native x86_64/ARM runners;
their source/build gates have run, while package installation remains a separate blocker.
Publication,
cryptographic provenance, SBOMs and immutable Actions pinning remain release work.

For future downloaded artifacts, verify the checksum file against an independently
trusted release source before extraction, then run `sha256sum -c SHA256SUMS`. A checksum
shipped alongside an untrusted archive detects corruption, not publisher authenticity.
There is intentionally no download helper that presents that as a signature guarantee.

## Uninstall

Stop and disable a user service you explicitly installed, remove that unit, and reload
the user manager. Then:

```sh
python3 packaging/install/uninstall.py
```

Only unchanged files matching the install manifest are removed. Changed binaries cause
a refusal, not blind deletion. Config, audit, project data, browser leftovers, portal
grants and GNOME/KWin/Blender add-ons are retained for explicit review/removal through
the appropriate application. Do not delete an active socket just to force shutdown.
