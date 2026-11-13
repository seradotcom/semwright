[CmdletBinding()]
param([string]$Prefix, [switch]$InPlace)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'Manage-Semwright.ps1') -Mode Install -Prefix $Prefix -InPlace:$InPlace
