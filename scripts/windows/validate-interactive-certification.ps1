param(
    [string]$Path = (Join-Path $PSScriptRoot "run-interactive-certification.ps1")
)

$ErrorActionPreference = "Stop"
$errors = $null
$tokens = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    $Path,
    [ref]$tokens,
    [ref]$errors
) | Out-Null

if ($errors.Count -ne 0) {
    $errors | ForEach-Object { Write-Error $_ }
    throw "Interactive certification PowerShell did not parse."
}
Write-Host "Interactive certification PowerShell parse: PASS"
. (Join-Path $PSScriptRoot "interactive-evidence-contract.ps1")

$checks = 0
function Expect-Refusal([scriptblock]$Action) {
    $refused = $false
    try { & $Action } catch { $refused = $true }
    if (-not $refused) { throw "Expected evidence/admission refusal." }
    $script:checks++
}
function Expect-Acceptance([scriptblock]$Action) {
    & $Action
    $script:checks++
}
$nl = [Environment]::NewLine
$one = @('running 1 test','test selected ... ok','test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.20s') -join $nl
$two = @('running 2 tests','test alpha ... ok','test beta ... ok','test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s') -join $nl
Expect-Acceptance { Assert-RustTestEvidence -Text $one -ExpectedTestName 'selected' }
Expect-Acceptance { Assert-RustTestEvidence -Text $two }
Expect-Acceptance { Assert-RustTestEvidence -Text ($one.Replace('ok', ([string][char]27 + '[32mok' + [char]27 + '[0m'))) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text '' }
Expect-Refusal { Assert-RustTestEvidence -Text 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.00s' }
Expect-Refusal { Assert-RustTestEvidence -Text $one -ExpectedTestName 'different' }
Expect-Refusal { Assert-RustTestEvidence -Text $one }
Expect-Refusal { Assert-RustTestEvidence -Text ($one.Replace('0 ignored','1 ignored')) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text ($one.Replace('0 failed','1 failed')) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text ($one.Replace('test selected ... ok','')) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text ($one + $nl + $one) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text ($two.Replace('test beta ... ok','test alpha ... ok')) }
Expect-Refusal { Assert-RustTestEvidence -Text ($one.Replace('test result: ok.','test result: FAILED.')) -ExpectedTestName 'selected' }
Expect-Refusal { Assert-RustTestEvidence -Text ($one.Replace('0 measured','1 measured')) -ExpectedTestName 'selected' }
Expect-Acceptance { Assert-InteractiveInvocation -Prebuilt $true -Case 'Semantic' -CaptureMode 'Skip' -DisposableConfirmed $false }
Expect-Acceptance { Assert-InteractiveInvocation -Prebuilt $true -Case 'InputPointerClipboard' -CaptureMode 'Skip' -DisposableConfirmed $true }
Expect-Acceptance { Assert-InteractiveInvocation -Prebuilt $false -Case 'Semantic' -CaptureMode 'Both' -DisposableConfirmed $true }
Expect-Refusal { Assert-InteractiveInvocation -Prebuilt $true -Case 'InputPointerClipboard' -CaptureMode 'Skip' -DisposableConfirmed $false }
Expect-Refusal { Assert-InteractiveInvocation -Prebuilt $false -Case 'Semantic' -CaptureMode 'Skip' -DisposableConfirmed $false }
Expect-Refusal { Assert-InteractiveInvocation -Prebuilt $true -Case 'Semantic' -CaptureMode 'Both' -DisposableConfirmed $true }
Expect-Refusal { Assert-InteractiveInvocation -Prebuilt $false -Case 'InputPointerClipboard' -CaptureMode 'Skip' -DisposableConfirmed $true }
Expect-Refusal { Assert-InteractiveInvocation -Prebuilt $false -Case 'Semantic' -CaptureMode 'Skip' -BinarySource ('a' * 40) -DisposableConfirmed $true }
Expect-Acceptance { Assert-PrebuiltSourceCompatibility -Root $PSScriptRoot -BinarySource ('a' * 40) -HarnessSource ('a' * 40) }
Expect-Refusal { Assert-PrebuiltSourceCompatibility -Root $PSScriptRoot -BinarySource 'not-a-sha' -HarnessSource ('a' * 40) }
if ($checks -ne 24) { throw "Unexpected regression check count: $checks" }
Write-Host "Interactive evidence/admission contract: $checks/24 PASS (no desktop I/O)"
