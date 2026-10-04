# Quick start

Semwright's native bundle installs the **core runtime as one matched version**. You do not need separate downloads for the CLI, daemon, MCP frontend, or terminal inspector.

The portable bundle contains:

| Command | Purpose |
| --- | --- |
| `semwright` | CLI and first-run setup |
| `semwrightd` | local broker/daemon |
| `semwright-mcp` | MCP frontend for compatible agent clients |
| `semwright-inspect` | read-only terminal UI / inspector |
| `semwright-sandbox` | platform isolation helper or fail-closed compatibility sentinel |

Application integrations have their own runtime requirements and evidence. Third-party applications, optional driver packages, and models are **not** silently downloaded by core setup.

> Pre-v1 candidate artifacts come from an exact-SHA V1 distribution run. A public Semwright release must use the same certified package set. Verify the release checksum before extracting a bundle.

## 1. Install and run safe setup

### Linux portable bundle

After extracting the matching `x86_64` or `aarch64` archive:

```sh
./install.sh
"$HOME/.local/share/semwright/bin/semwright" setup
```

On Debian/Ubuntu, the `.deb` is an alternative:

```sh
sudo apt install ./semwright_<version>_amd64.deb
semwright setup
```

Use the arm64 package on aarch64.

### Windows

After extracting the matching x86_64 or ARM64 ZIP:

```powershell
.\Install-Semwright.ps1
& "$env:LOCALAPPDATA\Semwright\bin\semwright.exe" setup
```

The installer is per-user and does not edit PATH, registry, services, or Windows security settings.

### macOS

After extracting the Apple Silicon (`arm64`) or Intel (`x86_64`) archive:

```sh
./install.sh
"$HOME/Library/Application Support/Semwright/bin/semwright" setup
```

The portable archive is unsigned/unnotarized until signing infrastructure is explicitly established; the helpers never disable Gatekeeper, SIP, or TCC.

`semwright setup` is deliberately safe and repeatable. It creates only a private observe-only config when missing and a ready-to-copy MCP client snippet. It preserves existing files and does **not** start a service, grant desktop/application permissions, approve sensitive actions, or edit a third-party MCP client.

Use `--dry-run --json setup` first if you want to inspect the paths without writing anything.

## 2. Start the broker and verify it

`semwright setup` prints the exact commands for your installation. With the default portable prefix:

### Linux

```sh
"$HOME/.local/share/semwright/bin/semwrightd" \
  --config "$HOME/.config/semwright/daemon.toml"
```

In another terminal:

```sh
"$HOME/.local/share/semwright/bin/semwright" --json doctor
"$HOME/.local/share/semwright/bin/semwright-inspect"
```

### Windows

```powershell
& "$env:LOCALAPPDATA\Semwright\bin\semwrightd.exe" `
  --config "$env:APPDATA\Semwright\config\daemon.toml"
```

In another terminal:

```powershell
& "$env:LOCALAPPDATA\Semwright\bin\semwright.exe" --json doctor
& "$env:LOCALAPPDATA\Semwright\bin\semwright-inspect.exe"
```

### macOS

```sh
"$HOME/Library/Application Support/Semwright/bin/semwrightd" \
  --config "$HOME/Library/Application Support/Semwright/config/daemon.toml"
```

In another terminal:

```sh
"$HOME/Library/Application Support/Semwright/bin/semwright" --json doctor
"$HOME/Library/Application Support/Semwright/bin/semwright-inspect"
```

Start with the observe-only policy. Live desktop/application access may require explicit OS consent or Semwright grants; setup never fabricates those approvals.

## 3. Connect an agent through MCP

Setup creates `mcp-client.json` with the exact installed `semwright-mcp` path:

| Platform | Generated snippet |
| --- | --- |
| Linux | `~/.config/semwright/mcp-client.json` |
| Windows | `%APPDATA%\Semwright\config\mcp-client.json` |
| macOS | `~/Library/Application Support/Semwright/config/mcp-client.json` |

Copy that entry into the compatible MCP client you choose. The MCP frontend does not start the broker or broaden policy.

## What should I install?

For normal use, install the **one native Semwright core bundle** for your OS and architecture. Splitting CLI/TUI/MCP into separately versioned downloads would create avoidable version-skew and support problems.

Use whichever interface you need after installation:

- terminal automation: `semwright`;
- agent integration: `semwright-mcp`;
- visual terminal inspection: `semwright-inspect`.

Then add only the application integrations you actually need. See [drivers](drivers.md) and the individual application guides for prerequisites, native runtime requirements, and current support.

## Remove or upgrade

Portable installs include a reversible uninstaller. Normal upgrades are intentionally conservative: stop the broker, uninstall the unchanged receipt-owned files, install the new bundle, then rerun `semwright setup`. Existing owner configuration is preserved and setup does not overwrite it.

For exact commands, checksum rules, `.deb` installation, custom prefixes, source builds, and security boundaries, continue with the full [installation guide](installation.md).
