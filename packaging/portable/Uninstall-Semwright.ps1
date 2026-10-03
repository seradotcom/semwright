[CmdletBinding()]
param([string]$Prefix)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'Manage-Semwright.ps1') -Mode Uninstall -Prefix $Prefix
