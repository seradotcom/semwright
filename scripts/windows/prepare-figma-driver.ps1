#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$DriverPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedSha256
)
$ErrorActionPreference = 'Stop'

function Assert-OrdinaryPath([string]$Path) {
    $cursor = [System.IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                throw "Reparse paths are not accepted: $cursor"
            }
        }
        $parent = [System.IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}

$source = [System.IO.Path]::GetFullPath($DriverPath)
Assert-OrdinaryPath $source
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Driver executable not found.' }
$expected = $ExpectedSha256.ToLowerInvariant()
if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'The owner-selected source digest does not match.'
}

$parent = Join-Path $env:LOCALAPPDATA 'Semwright\drivers\figma'
Assert-OrdinaryPath $parent
[void][System.IO.Directory]::CreateDirectory($parent)
$destination = Join-Path $parent ([guid]::NewGuid().ToString('N'))
if (Test-Path -LiteralPath $destination) { throw 'Destination already exists; refusing replacement.' }
$owner = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$acl = New-Object System.Security.AccessControl.DirectorySecurity
$acl.SetOwner($owner)
$acl.SetAccessRuleProtection($true, $false)
foreach ($sid in @($owner, (New-Object System.Security.Principal.SecurityIdentifier('S-1-5-18')))) {
    $rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
        $sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    [void]$acl.AddAccessRule($rule)
}
# Apply the DACL at creation, before any executable or manifest bytes enter the directory.
if ($PSVersionTable.PSEdition -eq 'Core') {
    [void][System.IO.FileSystemAclExtensions]::CreateDirectory($acl, $destination)
} else {
    [void][System.IO.Directory]::CreateDirectory($destination, $acl)
}
Assert-OrdinaryPath $destination
$actualAcl = Get-Acl -LiteralPath $destination
if (-not $actualAcl.AreAccessRulesProtected -or $actualAcl.Owner -ne $acl.Owner) {
    throw 'Destination did not retain its owner-controlled DACL.'
}
foreach ($rule in $actualAcl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
    if ($rule.AccessControlType -ne 'Allow' -or $rule.IdentityReference.Value -notin @($owner.Value, 'S-1-5-18')) {
        throw 'Destination has an unexpected access rule.'
    }
}
$installed = Join-Path $destination 'semwright-figma-driver.exe'
# CreateNew cannot overwrite an existing file or link inside the private directory.
$input = [System.IO.File]::OpenRead($source)
try {
    $output = [System.IO.File]::Open($installed, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $input.CopyTo($output); $output.Flush($true) } finally { $output.Dispose() }
} finally { $input.Dispose() }
if ((Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Installed driver digest changed during copying; do not enable it.'
}
$manifest = [ordered]@{
    manifest_version = 1; protocol = 2; id = 'figma'; version = '1.0.0'; publisher = 'semwright'
    executable = $installed; sha256 = $expected
    application = @{ desktop_id = $null; process_names = @('figma', 'Figma'); supported_versions = @() }
    transport = 'stdio_v1'; mounts = @(); system_config = @(); network = $false; loopback_port = 38471
    resources = @{ open_files = 128; processes = 32; cpu_seconds = 60; address_space_bytes = 536870912; file_size_bytes = 16777216 }
    request_timeout_ms = 30000
    interfaces = @{ dynamic_capabilities = $false; cooperative_cancellation = $false; events = $true; progress = $true; artifacts = $true; health = $true }
}
$manifestPath = Join-Path $destination 'driver.json'
[System.IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 12), (New-Object System.Text.UTF8Encoding($false)))
[pscustomobject]@{ executable = $installed; manifest = $manifestPath; sha256 = $expected; policy_grants_changed = $false } | ConvertTo-Json -Compress
