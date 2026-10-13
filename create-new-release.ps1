[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [string]$TargetBranch,

    [switch]$Yes
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

& "$PSScriptRoot\scripts\create-release.ps1" -Version $Version -TargetBranch $TargetBranch -Yes:$Yes
