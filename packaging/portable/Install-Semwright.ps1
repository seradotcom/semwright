[CmdletBinding()]
param([string]$Prefix)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'Manage-Semwright.ps1') -Mode Install -Prefix $Prefix
