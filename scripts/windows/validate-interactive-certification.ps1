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
