# Figma Bridge on Windows

This procedure requires a build containing the Windows approval-console and Host-loopback changes. The original 1.0.0 release does not implement Windows foreground approval. Installing that release again does not add these changes.

The Figma plugin connects to the Host's IPv4 loopback listener on port 38471. The sandboxed driver consumes the Host-selected Named Pipe, preserving the AppContainer boundary with `network=false`. No loopback exemption, firewall change or ambient Internet capability is required. Cloud REST operations require separate credentials and explicit network authority; this procedure configures the Plugin API route only.

## Prepare the driver

Use a trusted driver build and its independently verified SHA-256 digest. Run the repository helper as the same owner who runs the broker:

```powershell
$receipt = & .\scripts\windows\prepare-figma-driver.ps1 `
  -DriverPath 'C:\path\to\semwright-figma-driver.exe' `
  -ExpectedSha256 '<verified 64-character SHA-256>' | ConvertFrom-Json
$receipt.manifest
```

The helper copies the executable into a fresh owner/SYSTEM-only directory under `%LOCALAPPDATA%\Semwright\drivers\figma`, verifies the copied digest and writes a BOM-free manifest. It rejects reparse-point paths and refuses to replace an installation. It does not edit broker configuration or grant policy permissions. A digest calculated from an untrusted download is not an independent authenticity check.

In the owner's `daemon.toml`, append the returned manifest path to the top-level `drivers` array, preserving existing drivers and policy settings. TOML literal strings accept Windows backslashes directly:

```toml
drivers = ['C:\Users\OWNER\AppData\Local\Semwright\drivers\figma\INSTALL_ID\driver.json']
```

This entry belongs before `[policy]` or any other table header. Keep `driver_network=false` unless a separate driver already has an owner-approved network requirement. Local Figma bridging does not need that setting enabled.

## Start and pair

Stop the previous broker normally, then start the updated broker in a dedicated foreground PowerShell window:

```powershell
& 'C:\path\to\semwrightd.exe' --config "$env:APPDATA\Semwright\config\daemon.toml" --approval-console
```

Keep that window open. Services and detached processes are refused when `--approval-console` is selected. Approval reads `CONIN$` and writes `CONOUT$`; redirected stdin, MCP and Named Pipe requests cannot provide the affirmative response. Each request gets a fresh challenge, an exact ASCII answer and a 120-second deadline. Application text is escaped and buffered responses are discarded on exit.

In another PowerShell window, request the pairing code:

```powershell
& 'C:\path\to\semwright.exe' --json execute driver.figma.pairing.begin
```

If policy permits this operation and requires human confirmation, type the exact `approve <challenge>` response in the broker window. Keep the returned pairing code private.

Build `crates/driver-figma/plugin` with `npm ci --ignore-scripts` and `npm run build`. In Figma Desktop, import its `manifest.json` through Plugins > Development > Import plugin from manifest, open a disposable file, then run **Semwright Figma Bridge**. Enter the pairing code and use `ws://127.0.0.1:38471` as the bridge address. Verify the connection with `driver.figma.session.list` before authorizing mutations.

The helper does not broaden the default observe policy. Existing mutation and SecretAccess approval gates still apply. A policy denial requires an explicit owner policy decision, not a transport bypass.

## Automated evidence

The Windows platform workflow runs detached-startup rejection and a hidden, fixture-owned console against a `--fake` broker. It checks redirected-stdin rejection, exact approval, whitespace/Unicode/length rejection and challenge replay rejection. The Figma authoring workflow installs the production driver into a private directory, launches it through AppContainer with network disabled, authenticates an independent fake plugin through Host loopback and verifies an observable semantic mutation.

These fixtures do not certify a real Figma account, plugin UI, document or unlocked interactive desktop. Real application acceptance remains a separate disposable-file check.
