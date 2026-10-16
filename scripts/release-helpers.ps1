# Pure helpers for create-release.ps1 (no network, no git writes) so they can be tested anywhere:
#   pwsh -NoProfile -File scripts/release-helpers.test.ps1

$TagPattern = '^v(\d+)\.(\d+)\.(\d+)$'

function ConvertTo-VersionObject {
    param([Parameter(Mandatory)][string]$Tag)
    if ($Tag -match $TagPattern) {
        return [pscustomobject]@{
            Tag     = $Tag
            Version = [version]"$($Matches[1]).$($Matches[2]).$($Matches[3])"
        }
    }
    return $null
}

# Highest X.Y.Z tag. Burned tags (a failed release still occupies its tag) are included on purpose,
# so the suggestion is never an occupied version.
function Get-LatestTag {
    param([string[]]$Tags)
    $parsed = @($Tags | ForEach-Object { ConvertTo-VersionObject $_ } | Where-Object { $_ })
    if (-not $parsed) { return $null }
    return $parsed | Sort-Object -Property @{Expression = { $_.Version }; Descending = $true } | Select-Object -First 1
}

# Next suggestion: one patch past the highest tag (2.41.0 -> 2.41.1). Type a different version to jump.
function Get-NextVersion {
    param([string[]]$Tags)
    $latest = Get-LatestTag $Tags
    if (-not $latest) { throw "GitHub does not contain a version tag (vX.Y.Z)." }
    $v = $latest.Version
    return "$($v.Major).$($v.Minor).$($v.Build + 1)"
}

# Throws unless the version is X.Y.Z and sorts strictly after the latest existing tag.
function Assert-ReleaseVersion {
    param([Parameter(Mandatory)][string]$Version, [string[]]$Tags)
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        throw "Version '$Version' is not valid. Use X.Y.Z (the release audit requires it)."
    }
    $candidate = ConvertTo-VersionObject "v$Version"
    $latest = Get-LatestTag $Tags
    if ($latest -and -not ($candidate.Version -gt $latest.Version)) {
        throw "Version $Version is not newer than the latest tag $($latest.Tag)."
    }
}

function Test-ChangelogHasVersion {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Version)
    if (-not (Test-Path $Path)) { return $false }
    $pattern = '(?m)^## \[' + [regex]::Escape($Version) + '\]'
    return [bool](Select-String -Path $Path -Pattern $pattern -Quiet)
}

# Only this repository may be released from.
function Test-ReleaseOrigin {
    param([string]$Url)
    return [bool]($Url -match '[:/]tage-ilot/stagepilot(\.git)?/?$')
}
