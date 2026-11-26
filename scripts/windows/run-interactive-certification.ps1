param(
    [ValidateSet("Skip", "Select", "Cancel", "Both")]
    [string]$CaptureMode = "Skip",
    [string]$EvidenceRoot = "",
    [string]$PrebuiltUiaDirectory = "",
    [ValidateSet("Semantic", "InputPointerClipboard")]
    [string]$PrebuiltUiaCase = "Semantic",
    [string]$PrebuiltSource = "",
    [switch]$ConfirmDisposableDesktop
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "interactive-evidence-contract.ps1")
Assert-InteractiveInvocation -Prebuilt ([bool]$PrebuiltUiaDirectory) -Case $PrebuiltUiaCase -CaptureMode $CaptureMode -BinarySource $PrebuiltSource -DisposableConfirmed ([bool]$ConfirmDisposableDesktop)

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
$prebuiltUia = $null
$verifiedBinaryHash = $null
$binarySource = $sha
if ($PrebuiltUiaDirectory) {
    if ($PrebuiltSource) { $binarySource = $PrebuiltSource }
    Assert-PrebuiltSourceCompatibility -Root $Root -BinarySource $binarySource -HarnessSource $sha
    $prebuiltUia = (Resolve-Path -LiteralPath (Join-Path $PrebuiltUiaDirectory "uia-native-fixture.exe")).Path
    $manifestPath = Join-Path $PrebuiltUiaDirectory "manifest.json"
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.source -cne $binarySource -or $manifest.name -cne "uia-native-fixture.exe" -or
        $manifest.built_only -ne $true -or $manifest.interactive_accepted -ne $false) {
        throw "Prebuilt UIA manifest does not match this source and executable."
    }
    $verifiedBinaryHash = (Get-FileHash -LiteralPath $prebuiltUia -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($manifest.sha256 -notmatch '^[a-f0-9]{64}$' -or $verifiedBinaryHash -ne $manifest.sha256) {
        throw "Prebuilt UIA binary hash mismatch."
    }
}
if ((Test-Path -LiteralPath $EvidenceRoot) -and
    @(Get-ChildItem -LiteralPath $EvidenceRoot -Force).Count -gt 0) {
    throw "Interactive evidence directory must be empty."
}
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
        [string[]]$CargoArgs,
        [string]$ExpectedTestName
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
    try {
        Assert-RustTestEvidence -Text (Get-Content -LiteralPath $log -Raw) -ExpectedTestName $ExpectedTestName
        $Rows[$RowId].status = "PASS_WINDOWS_INTERACTIVE"
    } catch {
        $Rows[$RowId].status = "FAIL"
        throw
    }
}

function Invoke-PrebuiltUiaRow {
    $rowId = "uia_semantic"
    $testName = "real_win32_fixture_exercises_uia_without_pixel_fallback"
    $testArgs = @("--exact", $testName, "--show-output", "--test-threads=1")
    if ($PrebuiltUiaCase -eq "InputPointerClipboard") {
        $rowId = "input_pointer_clipboard"
        $testName = "interactive_windows_input_pointer_clipboard_and_focus_drift"
        $testArgs = @("--exact", $testName, "--ignored", "--show-output", "--test-threads=1")
    }
    $row = $Rows[$rowId]
    $log = Join-Path $EvidenceRoot "$rowId.log"
    $stderrLog = Join-Path $EvidenceRoot "$rowId.stderr.log"
    $row.log = (Split-Path $log -Leaf)
    $row.status = "RUNNING"
    $process = $null
    try {
        $start = @{
            FilePath = $prebuiltUia
            ArgumentList = $testArgs
            WorkingDirectory = $Root
            NoNewWindow = $true
            PassThru = $true
            RedirectStandardOutput = $log
            RedirectStandardError = $stderrLog
        }
        $process = Start-Process @start
        # Retain the process handle so Windows PowerShell can read its exit status.
        $null = $process.Handle
        if (-not $process.WaitForExit(45000)) {
            $process.Kill()
            [void]$process.WaitForExit(5000)
            throw "Prebuilt UIA case exceeded its 45-second physical-test budget."
        }
        $process.WaitForExit()
        $row.exit_code = $process.ExitCode
        $output = Get-Content -LiteralPath $log -Raw
        if ($row.exit_code -ne 0) {
            throw "Prebuilt interactive case failed; inspect its logs."
        }
        Assert-RustTestEvidence -Text $output -ExpectedTestName $testName
        $row.status = "PASS_WINDOWS_INTERACTIVE"
        Write-Host $output
    }
    catch {
        $row.status = "FAIL"
        throw
    }
    finally {
        if ($null -ne $process) {
            if (-not $process.HasExited) {
                $process.Kill()
                [void]$process.WaitForExit(5000)
            }
            $process.Dispose()
        }
    }
}

$failure = $null
try {
    if ($prebuiltUia) {
        Invoke-PrebuiltUiaRow
    } else {
        Invoke-CargoRow "uia_semantic" @(
            "test", "--locked", "-p", "semwright-platform-windows",
            "--test", "uia_native_fixture",
            "real_win32_fixture_exercises_uia_without_pixel_fallback",
            "--", "--exact", "--show-output", "--test-threads=1"
        ) "real_win32_fixture_exercises_uia_without_pixel_fallback"
        Invoke-CargoRow "input_pointer_clipboard" @(
            "test", "--locked", "-p", "semwright-platform-windows",
            "--test", "uia_native_fixture",
            "interactive_windows_input_pointer_clipboard_and_focus_drift",
            "--", "--exact", "--ignored", "--show-output", "--test-threads=1"
        ) "interactive_windows_input_pointer_clipboard_and_focus_drift"

        $authorityLog = Join-Path $EvidenceRoot "secure_spawn_authority.log"
        $Rows["secure_spawn_authority"].log = (Split-Path $authorityLog -Leaf)
        $Rows["secure_spawn_authority"].status = "RUNNING"
        $authorityCommands = @(
            @("test","--locked","-p","semwright-platform-windows-sys","--test","secure_spawn","--","--show-output","--test-threads=1"),
            @("test","--locked","-p","semwright-driver-host","--test","windows_secure_host","--","--show-output","--test-threads=1"),
            @("test","--locked","-p","semwright-plugin-host","--features","test-tools","--test","windows_secure_host","--","--show-output","--test-threads=1"),
            @("test","--locked","-p","semwright-federation","--test","windows_secure_stdio","--","--show-output","--test-threads=1")
        )
        foreach ($cargoArgs in $authorityCommands) {
            "COMMAND: cargo $($cargoArgs -join ' ')" | Tee-Object -FilePath $authorityLog -Append
            # Each individual suite must pass. One passing suite cannot
            # conceal a zero-test or incomplete suite in the same row.
            $suiteLog = Join-Path $EvidenceRoot ("secure_spawn_authority-" + $cargoArgs[3] + ".log")
            & cargo @cargoArgs 2>&1 | Tee-Object -FilePath $suiteLog
            $code = $LASTEXITCODE
            Get-Content -LiteralPath $suiteLog | Add-Content -LiteralPath $authorityLog
            if ($code -ne 0) {
                $Rows["secure_spawn_authority"].exit_code = $code
                $Rows["secure_spawn_authority"].status = "FAIL"
                throw "Secure-spawn authority certification failed."
            }
            try {
                Assert-RustTestEvidence -Text (Get-Content -LiteralPath $suiteLog -Raw)
            } catch {
                $Rows["secure_spawn_authority"].status = "FAIL"
                throw
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
                "--", "--exact", "--ignored", "--show-output", "--test-threads=1"
            ) "interactive_capture_picker_selects_bounded_png"
        }
        if ($CaptureMode -in @("Cancel", "Both")) {
            Write-Host "ACTION REQUIRED: cancel the Windows capture picker."
            Invoke-CargoRow "capture_cancel" @(
                "test", "--locked", "-p", "semwright-platform-windows",
                "--test", "capture_interactive",
                "interactive_capture_picker_user_cancel_is_cancelled",
                "--", "--exact", "--ignored", "--show-output", "--test-threads=1"
            ) "interactive_capture_picker_user_cancel_is_cancelled"
        }
    }
}
catch {
    $failure = $_.Exception.Message
}
finally {
    $system = [ordered]@{
        sha = $binarySource
        harness_sha = $sha
        disposable_desktop_confirmed = [bool]$ConfirmDisposableDesktop
        utc = (Get-Date).ToUniversalTime().ToString("o")
        machine = $env:COMPUTERNAME
        architecture = $env:PROCESSOR_ARCHITECTURE
        session_id = $currentProcess.SessionId
        github_actions = $env:GITHUB_ACTIONS
        runner_environment = $env:RUNNER_ENVIRONMENT
        capture_mode = $CaptureMode
        mode = $(if ($prebuiltUia -and $PrebuiltUiaCase -eq "InputPointerClipboard") {
            "prebuilt-input-pointer-clipboard-only"
        } elseif ($prebuiltUia) { "prebuilt-uia-only" } else { "cargo-certification" })
        binary_sha256 = $verifiedBinaryHash
    }
    $hashes = @()
    Get-ChildItem $EvidenceRoot -Filter "*.log" -File | ForEach-Object {
        $hash = Get-FileHash -Algorithm SHA256 $_.FullName
        $hashes += [ordered]@{ file = $_.Name; sha256 = $hash.Hash.ToLowerInvariant() }
    }
    $failed = @($Rows.Values | Where-Object { $_.status -eq "FAIL" })
    $pending = @($Rows.Values | Where-Object { $_.status -ne "PASS_WINDOWS_INTERACTIVE" })
    $classification = if ($failure -or $failed.Count -gt 0) {
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
