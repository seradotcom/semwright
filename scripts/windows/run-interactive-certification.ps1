param(
    [ValidateSet("Skip", "Select", "Cancel", "Both")]
    [string]$CaptureMode = "Skip",
    [string]$EvidenceRoot = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Set-Location $Root
if (-not $EvidenceRoot) {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $EvidenceRoot = Join-Path $Root "verification/windows-interactive/$stamp"
}

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw "Windows interactive certification can only run on Windows."
}
if (-not [System.Environment]::UserInteractive) {
    throw "Windows interactive certification requires an interactive user session."
}
if ($env:GITHUB_ACTIONS -eq "true" -and $env:RUNNER_ENVIRONMENT -ne "self-hosted") {
    throw "GitHub-hosted runners must never claim PASS_WINDOWS_INTERACTIVE."
}
$currentProcess = Get-Process -Id $PID
$explorer = Get-Process explorer -ErrorAction SilentlyContinue |
    Where-Object { $_.SessionId -eq $currentProcess.SessionId }
if (-not $explorer) {
    throw "No Explorer shell is running in the current session; unlock the disposable desktop."
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class SemwrightInteractiveNative {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
"@
if ([SemwrightInteractiveNative]::GetForegroundWindow() -eq [IntPtr]::Zero) {
    throw "No foreground window is visible on the interactive input desktop."
}

$dirty = git status --porcelain
if ($LASTEXITCODE -ne 0) { throw "git status failed" }
if ($dirty) {
    throw "Interactive certification requires a clean checkout."
}
$sha = (git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed" }
New-Item -ItemType Directory -Force -Path $EvidenceRoot | Out-Null
$env:SEMWRIGHT_WINDOWS_INTERACTIVE = "1"
$Rows = [ordered]@{}
function New-Row([string]$Id, [string]$Description) {
    $Rows[$Id] = [ordered]@{
        id = $Id
        description = $Description
        status = "WINDOWS_INTERACTIVE_PENDING"
        log = $null
        exit_code = $null
    }
}

New-Row "uia_semantic" "UIA snapshot/hit-test/invoke/password redaction/stale refs"
New-Row "input_pointer_clipboard" "Unicode input, focus drift, pointer click/move, clipboard bounds"
New-Row "secure_spawn_authority" "Driver/Plugin/MCP AppContainer authority profile"
New-Row "capture_select" "WGC system picker selection and bounded PNG"
New-Row "capture_cancel" "WGC system picker user cancellation"
New-Row "uia_events_virtualization" "Native UIA events, virtualized controls and large trees"
New-Row "uipi_elevated_negative" "Normal process cannot synthesize into elevated target"
New-Row "uac_secure_desktop" "No interaction with UAC secure desktop"
New-Row "mixed_dpi_multimonitor" "96/125/150/200 percent, mixed DPI and negative coordinates"
New-Row "session_lock_sleep" "Lock/unlock and sleep/wake invalidation/reprobe"
New-Row "ipc_negative" "Wrong SID/session, remote client and impersonation reversion"
New-Row "real_apps" "Notepad, Calculator, Explorer, WinUI and Chromium matrix"
function Invoke-CargoRow {
    param(
        [string]$RowId,
        [string[]]$CargoArgs
    )
    $log = Join-Path $EvidenceRoot "$RowId.log"
    $Rows[$RowId].log = (Split-Path $log -Leaf)
    $Rows[$RowId].status = "RUNNING"
    "COMMAND: cargo $($CargoArgs -join ' ')" | Tee-Object -FilePath $log
    & cargo @CargoArgs 2>&1 | Tee-Object -FilePath $log -Append
    $code = $LASTEXITCODE
    $Rows[$RowId].exit_code = $code
    if ($code -ne 0) {
        $Rows[$RowId].status = "FAIL"
        throw "Interactive certification row '$RowId' failed with exit code $code."
    }
    $Rows[$RowId].status = "PASS_WINDOWS_INTERACTIVE"
}

$failure = $null
try {
    Invoke-CargoRow "uia_semantic" @(
        "test", "--locked", "-p", "semwright-platform-windows",
        "--test", "uia_native_fixture",
        "real_win32_fixture_exercises_uia_without_pixel_fallback",
        "--", "--nocapture", "--test-threads=1"
    )
    Invoke-CargoRow "input_pointer_clipboard" @(
        "test", "--locked", "-p", "semwright-platform-windows",
        "--test", "uia_native_fixture",
        "interactive_windows_input_pointer_clipboard_and_focus_drift",
        "--", "--ignored", "--nocapture", "--test-threads=1"
    )

    $authorityLog = Join-Path $EvidenceRoot "secure_spawn_authority.log"
    $Rows["secure_spawn_authority"].log = (Split-Path $authorityLog -Leaf)
    $Rows["secure_spawn_authority"].status = "RUNNING"
    $authorityCommands = @(
        @("test","--locked","-p","semwright-platform-windows-sys","--test","secure_spawn","--","--nocapture","--test-threads=1"),
        @("test","--locked","-p","semwright-driver-host","--test","windows_secure_host","--","--nocapture","--test-threads=1"),
        @("test","--locked","-p","semwright-plugin-host","--features","test-tools","--test","windows_secure_host","--","--nocapture","--test-threads=1"),
        @("test","--locked","-p","semwright-federation","--test","windows_secure_stdio","--","--nocapture","--test-threads=1")
    )
    foreach ($cargoArgs in $authorityCommands) {
        "COMMAND: cargo $($cargoArgs -join ' ')" | Tee-Object -FilePath $authorityLog -Append
        & cargo @cargoArgs 2>&1 | Tee-Object -FilePath $authorityLog -Append
        if ($LASTEXITCODE -ne 0) {
            $Rows["secure_spawn_authority"].exit_code = $LASTEXITCODE
            $Rows["secure_spawn_authority"].status = "FAIL"
            throw "Secure-spawn authority certification failed."
        }
    }
    $Rows["secure_spawn_authority"].exit_code = 0
    $Rows["secure_spawn_authority"].status = "PASS_WINDOWS_INTERACTIVE"

    if ($CaptureMode -in @("Select", "Both")) {
        Write-Host "ACTION REQUIRED: select a non-sensitive window in the Windows capture picker."
        Invoke-CargoRow "capture_select" @(
            "test", "--locked", "-p", "semwright-platform-windows",
            "--test", "capture_interactive",
            "interactive_capture_picker_selects_bounded_png",
            "--", "--ignored", "--nocapture", "--test-threads=1"
        )
    }
    if ($CaptureMode -in @("Cancel", "Both")) {
        Write-Host "ACTION REQUIRED: cancel the Windows capture picker."
        Invoke-CargoRow "capture_cancel" @(
            "test", "--locked", "-p", "semwright-platform-windows",
            "--test", "capture_interactive",
            "interactive_capture_picker_user_cancel_is_cancelled",
            "--", "--ignored", "--nocapture", "--test-threads=1"
        )
    }
}
catch {
    $failure = $_.Exception.Message
}
finally {
    $system = [ordered]@{
        sha = $sha
        utc = (Get-Date).ToUniversalTime().ToString("o")
        machine = $env:COMPUTERNAME
        architecture = $env:PROCESSOR_ARCHITECTURE
        session_id = $currentProcess.SessionId
        github_actions = $env:GITHUB_ACTIONS
        runner_environment = $env:RUNNER_ENVIRONMENT
        capture_mode = $CaptureMode
    }
    $hashes = @()
    Get-ChildItem $EvidenceRoot -Filter "*.log" -File | ForEach-Object {
        $hash = Get-FileHash -Algorithm SHA256 $_.FullName
        $hashes += [ordered]@{ file = $_.Name; sha256 = $hash.Hash.ToLowerInvariant() }
    }
    $failed = @($Rows.Values | Where-Object { $_.status -eq "FAIL" })
    $pending = @($Rows.Values | Where-Object { $_.status -eq "WINDOWS_INTERACTIVE_PENDING" })
    $classification = if ($failed.Count -gt 0) {
        "FAIL"
    } elseif ($pending.Count -gt 0) {
        "WINDOWS_INTERACTIVE_PENDING"
    } else {
        "PASS_WINDOWS_INTERACTIVE"
    }
    $result = [ordered]@{
        classification = $classification
        failure = $failure
        system = $system
        rows = @($Rows.Values)
        log_hashes = $hashes
        note = "Only rows explicitly marked PASS_WINDOWS_INTERACTIVE have interactive evidence; all others remain pending."
    }
    $result | ConvertTo-Json -Depth 8 |
        Set-Content -Encoding UTF8 (Join-Path $EvidenceRoot "result.json")
    Write-Host "Evidence: $EvidenceRoot"
    Write-Host "Overall classification: $($result.classification)"
}
if ($failure) { throw $failure }
