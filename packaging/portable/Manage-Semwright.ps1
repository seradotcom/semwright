[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Install','Uninstall')][string]$Mode,
    [string]$Prefix,
    [switch]$InPlace,
    [switch]$Check
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($Check -and $Mode -ne 'Install') { throw '-Check is only supported for installation.' }
$Here = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\','/')
if ($InPlace) {
    if ($Mode -ne 'Install' -or $Prefix) { throw 'Use -InPlace for installation without -Prefix.' }
    $Prefix = $Here
} elseif (-not $Prefix) {
    if ($Mode -eq 'Uninstall' -and (Test-Path -LiteralPath (Join-Path $Here '.semwright-install-receipt'))) {
        $Prefix = $Here
    } else {
        if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is required for user-local installation.' }
        $Prefix = Join-Path $env:LOCALAPPDATA 'Semwright'
    }
}
$Prefix = [IO.Path]::GetFullPath($Prefix).TrimEnd('\','/')
$UserRoot = [IO.Path]::GetFullPath([Environment]::GetFolderPath('UserProfile')).TrimEnd('\','/')
$InUserProfile = $Prefix.StartsWith($UserRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)
$OwnUninstall = $Mode -eq 'Uninstall' -and $Prefix.Equals($Here, [StringComparison]::OrdinalIgnoreCase)
if ($InPlace -or (-not $InUserProfile -and $OwnUninstall)) {
    # Outside the profile, manage only the verified bundle containing this helper.
    if ($Prefix -notmatch '^[A-Za-z]:\\[^\\]' -or
        ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($Prefix))).DriveType -notin @('Fixed','Removable')) {
        throw 'In-place bundles require a non-root directory on an ordinary local drive.'
    }
} elseif (-not $InUserProfile) {
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
function Assert-NoRunningBundleProcesses([string]$Base) {
    $BinRoot = [IO.Path]::GetFullPath((Join-Path $Base 'bin')).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    $Names = @('semwright.exe', 'semwrightd.exe', 'semwright-mcp.exe', 'semwright-inspect.exe', 'semwright-sandbox.exe')
    $Filter = ($Names | ForEach-Object { "Name='$_'" }) -join ' OR '
    try {
        $Processes = @(Get-CimInstance -ClassName Win32_Process -Filter $Filter -ErrorAction Stop)
    } catch {
        throw 'Cannot inspect running Semwright processes; refusing uninstall.'
    }
    foreach ($Process in $Processes) {
        if ([string]::IsNullOrWhiteSpace($Process.ExecutablePath)) {
            throw 'Cannot confirm the location of a running Semwright process; refusing uninstall.'
        }
        $Executable = [IO.Path]::GetFullPath($Process.ExecutablePath)
        if ($Executable.StartsWith($BinRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Stop running Semwright from this bundle before uninstalling (PID $($Process.ProcessId))."
        }
    }
}
# This read-only preflight mirrors the runtime's installation ACL boundary.
# It is not a path lock or a substitute for the MCP handle-based peer checks.
$InstallOwnerSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$CheckedInstallDirectories = @{}
function Assert-InstallationAcl([string]$Path, [switch]$Private, [switch]$InheritedChild) {
    function Deny-InstallPath([string]$Reason) {
        throw "Untrusted Windows installation path: $Path ($Reason). Choose an owner-protected location; existing permissions were not changed."
    }
    try {
        $Acl = Get-Acl -LiteralPath $Path -ErrorAction Stop
        $Descriptor = [Security.AccessControl.RawSecurityDescriptor]::new($Acl.GetSecurityDescriptorBinaryForm(), 0)
    } catch {
        Deny-InstallPath 'security descriptor could not be read'
    }
    $VolumeRoot = [string]::IsNullOrEmpty([IO.Path]::GetDirectoryName($Path))
    $Trusted = @($InstallOwnerSid, 'S-1-5-18', 'S-1-5-32-544')
    $TrustedInstaller = 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464'
    if ($null -eq $Descriptor.Owner -or
        ($Descriptor.Owner.Value -notin $Trusted -and
         -not ($VolumeRoot -and $Descriptor.Owner.Value -eq $TrustedInstaller))) {
        Deny-InstallPath 'owner is not trusted'
    }
    if ($null -eq $Descriptor.DiscretionaryAcl -or $Descriptor.DiscretionaryAcl.Count -gt 4096) {
        Deny-InstallPath 'missing or oversized discretionary ACL'
    }
    # Actual write/delete/ACL mutation bits; ordinary read/execute is not write.
    $Restricted = [long]0x500D0156
    if ($Private -or $InheritedChild) {
        # The broker treats checksums and receipts as private data.
        $Restricted = $Restricted -bor [long]0x00120089 -bor [long]2147483648
    } elseif ($VolumeRoot) {
        # A volume root may allow creating a new sibling directory, not deletion.
        $Restricted = $Restricted -band (-bnot [long]4)
    }
    $Allowed = $Trusted + @('S-1-3-4')
    if ($VolumeRoot -and -not $Private -and -not $InheritedChild) {
        $Allowed += $TrustedInstaller
    }
    if ($InheritedChild) {
        # CREATOR OWNER in an inheritance template becomes the new file's owner.
        $Allowed += 'S-1-3-0'
    }
    foreach ($Ace in $Descriptor.DiscretionaryAcl) {
        $Type = [int]$Ace.AceType
        if ($Type -in @(1, 6, 10, 12)) { continue }
        if ($Type -notin @(0, 5, 9, 11)) { Deny-InstallPath 'unsupported ACL entry' }
        if (($Ace.AccessMask -band $Restricted) -eq 0) { continue }
        if ($InheritedChild -and ([int]$Ace.AceFlags -band 3) -eq 0) { continue }
        if ($Type -ne 0) { Deny-InstallPath 'complex restricted allow entry' }
        if (-not $InheritedChild -and ([int]$Ace.AceFlags -band 8) -ne 0) { continue }
        if ($Ace.SecurityIdentifier.Value -notin $Allowed) {
            Deny-InstallPath 'restricted rights are granted to another account or group'
        }
    }
}
function Assert-InstallationAncestors([string]$Path) {
    $Directories = [Collections.Generic.List[string]]::new()
    $Current = [IO.Path]::GetFullPath($Path)
    while ($Current) {
        if ($Directories.Count -ge 64) { throw 'Installation path exceeds the directory limit.' }
        $Directories.Add($Current)
        $Current = [IO.Path]::GetDirectoryName($Current)
    }
    # Report the outermost cause, since protecting only an inner directory is insufficient.
    for ($Index = $Directories.Count - 1; $Index -ge 0; $Index--) {
        $Directory = $Directories[$Index]
        if (-not $CheckedInstallDirectories.ContainsKey($Directory) -and
            (Test-Path -LiteralPath $Directory)) {
            if (-not (Get-Item -LiteralPath $Directory -Force).PSIsContainer) {
                throw "Expected an installation directory: $Directory"
            }
            Assert-NoReparse $Directory
            Assert-InstallationAcl $Directory
            $CheckedInstallDirectories[$Directory] = $true
        }
    }
}
function Assert-PrivateInstallInheritance([string]$Path) {
    $Parent = $Path
    while (-not (Test-Path -LiteralPath $Parent)) {
        $Parent = [IO.Path]::GetDirectoryName($Parent)
        if (-not $Parent) { throw 'Cannot determine installation parent permissions.' }
    }
    Assert-InstallationAcl $Parent -InheritedChild
}

Assert-NoReparse $Prefix
$Receipt = Join-Path $Prefix '.semwright-install-receipt'
if ($Mode -eq 'Install') {
    if (-not $InPlace -and (Test-Path -LiteralPath $Prefix)) {
        throw 'Refusing to replace an existing directory; review/uninstall first.'
    }
    if ($InPlace -and -not $Check -and (Test-Path -LiteralPath $Receipt)) {
        throw 'Refusing to replace an existing installation receipt; review/uninstall first.'
    }
    $Checksums = Join-Path $Here 'SHA256SUMS'
    $Records = @(Read-Records $Checksums)
    Assert-Files $Here $Records
    foreach ($Name in @('semwright','semwrightd','semwright-mcp','semwright-inspect','semwright-sandbox')) {
        if (@($Records | Where-Object { $_.Path -ceq "bin/$Name.exe" }).Count -ne 1) {
            throw "Missing checksummed executable: $Name"
        }
    }
    Assert-InstallationAncestors $Here
    Assert-InstallationAncestors $Prefix
    foreach ($Record in $Records) {
        $Source = Join-Path $Here $Record.Path
        Assert-InstallationAncestors ([IO.Path]::GetDirectoryName($Source))
        Assert-InstallationAcl $Source
    }
    Assert-InstallationAcl $Checksums -Private:$InPlace
    Assert-PrivateInstallInheritance $Prefix
    if ($Check) {
        if ($InPlace -and (Test-Path -LiteralPath $Receipt)) {
            Assert-InstallationAcl $Receipt -Private
            Assert-Files $Prefix @(Read-Records $Receipt)
        }
        Write-Output "Installation preflight passed for $Prefix. No files or settings were changed."
        return
    }
    if (-not $InPlace) {
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
    }
    $Lines = @([IO.File]::ReadAllLines($Checksums)) + @((Get-Digest $Checksums) + '  SHA256SUMS')
    # CreateNew also refuses a receipt created by a concurrent installer.
    $Stream = [IO.File]::Open($Receipt, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $Writer = [IO.StreamWriter]::new($Stream, [Text.UTF8Encoding]::new($false))
        try { foreach ($Line in $Lines) { $Writer.WriteLine($Line) } }
        finally { $Writer.Dispose() }
    } finally { $Stream.Dispose() }
    Assert-Files $Prefix @(Read-Records $Receipt)
    Assert-InstallationAcl (Join-Path $Prefix 'SHA256SUMS') -Private
    Assert-InstallationAcl $Receipt -Private
    if ($InPlace) { Write-Output "Registered verified portable files in $Prefix" }
    else { Write-Output "Installed in $Prefix" }
    Write-Output 'PATH, registry, services, configuration and Windows security controls were not changed.'
    # Use PowerShell's escaper, including its typographic single-quote delimiters.
    $CliLiteral = "'" + [System.Management.Automation.Language.CodeGeneration]::EscapeSingleQuotedStringContent((Join-Path $Prefix 'bin\semwright.exe')) + "'"
    $UninstallLiteral = "'" + [System.Management.Automation.Language.CodeGeneration]::EscapeSingleQuotedStringContent((Join-Path $Prefix 'Uninstall-Semwright.ps1')) + "'"
    Write-Output "Next: & $CliLiteral setup"
    Write-Output "Then start the broker and run: & $CliLiteral --json doctor"
    Write-Output "Remove: & $UninstallLiteral"
} else {
    $Records = @(Read-Records $Receipt)
    Assert-Files $Prefix $Records
    Assert-NoRunningBundleProcesses $Prefix
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
