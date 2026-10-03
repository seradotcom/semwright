[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Install','Uninstall')][string]$Mode,
    [string]$Prefix
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Here = $PSScriptRoot
if (-not $Prefix) {
    if ($Mode -eq 'Uninstall' -and (Test-Path -LiteralPath (Join-Path $Here '.semwright-install-receipt'))) {
        $Prefix = $Here
    } else {
        if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is required for user-local installation.' }
        $Prefix = Join-Path $env:LOCALAPPDATA 'Semwright'
    }
}
$Prefix = [IO.Path]::GetFullPath($Prefix).TrimEnd('\','/')
$UserRoot = [IO.Path]::GetFullPath([Environment]::GetFolderPath('UserProfile')).TrimEnd('\','/')
if (-not $Prefix.StartsWith($UserRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Install prefix must be a child of your user profile; no system-wide installation.'
}
function Assert-NoReparse([string]$Path) {
    $Current = [IO.Path]::GetFullPath($Path)
    while ($Current) {
        if (Test-Path -LiteralPath $Current) {
            $Item = Get-Item -LiteralPath $Current -Force
            if (($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Reparse point refused: $Current"
            }
        }
        $Parent = [IO.Path]::GetDirectoryName($Current)
        if ($Parent -eq $Current) { break }
        $Current = $Parent
    }
}
function Get-Digest([string]$Path) {
    Assert-NoReparse $Path
    $Item = Get-Item -LiteralPath $Path -Force
    if ($Item.PSIsContainer) { throw "Expected a regular file: $Path" }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Read-Records([string]$Path) {
    Assert-NoReparse $Path
    $Item = Get-Item -LiteralPath $Path -Force
    if ($Item.PSIsContainer -or $Item.Length -gt 131072) { throw 'Invalid or oversized receipt.' }
    $Rows = @([IO.File]::ReadAllLines($Path))
    if ($Rows.Count -lt 1 -or $Rows.Count -gt 1024) { throw 'Invalid receipt entry count.' }
    $Seen = @{}
    foreach ($Line in $Rows) {
        if ($Line -cnotmatch '^([0-9a-f]{64})  ([A-Za-z0-9._/-]+)$') { throw 'Malformed receipt entry.' }
        $Digest = $Matches[1]; $Relative = $Matches[2]
        if ($Relative.StartsWith('/') -or $Relative -eq '.semwright-install-receipt' -or
            @($Relative.Split('/') | Where-Object { $_ -eq '.' -or $_ -eq '..' -or $_ -eq '' }).Count -ne 0) {
            throw 'Unsafe receipt path.'
        }
        # Windows filenames are case-insensitive; duplicates are rejected accordingly.
        if ($Seen.ContainsKey($Relative)) { throw 'Duplicate receipt path.' }
        $Seen[$Relative] = $true
        [pscustomobject]@{ Path=$Relative; Digest=$Digest }
    }
}
function Assert-Files([string]$Base, [object[]]$Records) {
    foreach ($Record in $Records) {
        $Target = Join-Path $Base $Record.Path
        if ((Get-Digest $Target) -cne $Record.Digest) { throw "File changed; refusing operation: $Target" }
    }
}
Assert-NoReparse $Prefix
$Receipt = Join-Path $Prefix '.semwright-install-receipt'
if ($Mode -eq 'Install') {
    if (Test-Path -LiteralPath $Prefix) { throw 'Refusing to replace an existing directory; review/uninstall first.' }
    $Checksums = Join-Path $Here 'SHA256SUMS'
    $Records = @(Read-Records $Checksums)
    Assert-Files $Here $Records
    foreach ($Name in @('semwright','semwrightd','semwright-mcp','semwright-inspect','semwright-sandbox')) {
        if (@($Records | Where-Object { $_.Path -ceq "bin/$Name.exe" }).Count -ne 1) {
            throw "Missing checksummed executable: $Name"
        }
    }
    $Parent = [IO.Path]::GetDirectoryName($Prefix)
    [void][IO.Directory]::CreateDirectory($Parent)
    Assert-NoReparse $Parent
    # New-Item without Force refuses an existing leaf. No unknown files are overwritten.
    [void](New-Item -ItemType Directory -Path $Prefix)
    foreach ($Record in $Records) {
        $Target = Join-Path $Prefix $Record.Path
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Target))
        [IO.File]::Copy((Join-Path $Here $Record.Path), $Target, $false)
    }
    [IO.File]::Copy($Checksums, (Join-Path $Prefix 'SHA256SUMS'), $false)
    $Lines = @([IO.File]::ReadAllLines($Checksums)) + @((Get-Digest $Checksums) + '  SHA256SUMS')
    [IO.File]::WriteAllLines($Receipt, [string[]]$Lines, [Text.UTF8Encoding]::new($false))
    Assert-Files $Prefix @(Read-Records $Receipt)
    Write-Output "Installed in $Prefix"
    Write-Output 'PATH, registry, services, configuration and Windows security controls were not changed.'
    Write-Output "Try: & '$Prefix\bin\semwright.exe' --help"
    Write-Output "Remove: & '$Prefix\Uninstall-Semwright.ps1'"
} else {
    $Records = @(Read-Records $Receipt)
    Assert-Files $Prefix $Records
    # Validate every file before any deletion; no recursive removal or wildcard expansion.
    foreach ($Record in $Records) {
        $Target = Join-Path $Prefix $Record.Path
        if ((Get-Digest $Target) -cne $Record.Digest) { throw "File changed during removal: $Target" }
        [IO.File]::Delete($Target)
    }
    [IO.File]::Delete($Receipt)
    foreach ($Record in $Records) {
        $Parent = [IO.Path]::GetDirectoryName((Join-Path $Prefix $Record.Path))
        while ($Parent -and -not $Parent.Equals($Prefix, [StringComparison]::OrdinalIgnoreCase)) {
            if ((Test-Path -LiteralPath $Parent) -and @([IO.Directory]::EnumerateFileSystemEntries($Parent)).Count -eq 0) {
                [IO.Directory]::Delete($Parent, $false)
            }
            $Parent = [IO.Path]::GetDirectoryName($Parent)
        }
    }
    if (@([IO.Directory]::EnumerateFileSystemEntries($Prefix)).Count -eq 0) {
        [IO.Directory]::Delete($Prefix, $false)
    } else { Write-Output "Retained unowned files in $Prefix" }
    Write-Output 'Removed only receipt-owned unchanged files. External user data and settings were retained.'
}
