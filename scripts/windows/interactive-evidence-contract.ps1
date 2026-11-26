# Pure evidence and admission checks. Sourcing this file does not run a test.
function Assert-InteractiveInvocation {
    param(
        [bool]$Prebuilt,
        [string]$Case,
        [string]$CaptureMode,
        [string]$BinarySource,
        [bool]$DisposableConfirmed
    )
    if ($Prebuilt -and $CaptureMode -ne 'Skip') {
        throw 'Prebuilt UIA/input mode cannot run the capture picker.'
    }
    if (-not $Prebuilt -and ($Case -ne 'Semantic' -or $BinarySource)) {
        throw 'Prebuilt case/source options require a prebuilt UIA directory.'
    }
    if ((-not $Prebuilt -or $Case -eq 'InputPointerClipboard') -and -not $DisposableConfirmed) {
        throw 'This case changes focus, cursor or clipboard. Explicitly confirm an unlocked disposable desktop.'
    }
}

function Assert-RustTestEvidence {
    param(
        [Parameter(Mandatory=$true)][AllowEmptyString()][string]$Text,
        [string]$ExpectedTestName = ''
    )
    $plain = [regex]::Replace($Text, '\x1b\[[0-?]*[ -/]*[@-~]', '')
    $summaries = [regex]::Matches($plain, '(?m)^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;[^\r\n]*\r?$')
    if ($summaries.Count -ne 1) { throw 'Expected exactly one complete Rust test summary.' }
    $summary = $summaries[0]
    $passed = [int]$summary.Groups[2].Value
    if ($summary.Groups[1].Value -cne 'ok' -or $passed -lt 1 -or
        [int]$summary.Groups[3].Value -ne 0 -or
        [int]$summary.Groups[4].Value -ne 0 -or
        [int]$summary.Groups[5].Value -ne 0) {
        throw 'No complete passing test evidence, or failed/ignored/measured tests remain.'
    }
    $completed = [regex]::Matches($plain, '(?m)^test ([^\r\n]+) \.\.\. ok[ \t]*\r?$')
    $names = @($completed | ForEach-Object { $_.Groups[1].Value })
    if ($completed.Count -ne $passed -or @($names | Sort-Object -Unique).Count -ne $passed) {
        throw 'Passing test names do not match the Rust test summary.'
    }
    if ($ExpectedTestName) {
        if ($passed -ne 1 -or $names[0] -cne $ExpectedTestName) {
            throw "Expected exactly the selected passing test: $ExpectedTestName"
        }
    } elseif ([int]$summary.Groups[6].Value -ne 0) {
        throw 'An unfiltered authority suite must not omit tests.'
    }
}

function Assert-PrebuiltSourceCompatibility {
    param(
        [Parameter(Mandatory=$true)][string]$Root,
        [Parameter(Mandatory=$true)][string]$BinarySource,
        [Parameter(Mandatory=$true)][string]$HarnessSource
    )
    if ($BinarySource -cnotmatch '^[a-f0-9]{40}$' -or $HarnessSource -cnotmatch '^[a-f0-9]{40}$') {
        throw 'Exact lowercase source commits are required.'
    }
    if ($BinarySource -ceq $HarnessSource) { return }
    # A verified archived binary may be reused only for a harness-only
    # descendant. No compiled source, tests, dependencies or other files
    # may differ from its actual source commit.
    & git -C $Root merge-base --is-ancestor $BinarySource $HarnessSource
    if ($LASTEXITCODE -ne 0) { throw 'Archived binary source is not an ancestor of the harness.' }
    $changed = @(& git -C $Root diff --no-ext-diff --name-only --no-renames $BinarySource $HarnessSource --)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot compare archived source with harness.' }
    $harnessOnly = @(
        'scripts/windows/run-interactive-certification.ps1',
        'scripts/windows/interactive-evidence-contract.ps1',
        'scripts/windows/validate-interactive-certification.ps1',
        '.github/workflows/windows-interactive.yml',
        'docs/windows/WINDOWS_VERIFY.md'
    )
    foreach ($path in $changed) {
        if ($path -cnotin $harnessOnly) { throw "Archived binary is not source-equivalent: $path" }
    }
}
