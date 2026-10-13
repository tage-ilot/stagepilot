# Pure helpers for create-release.ps1 (no network, no git writes) so they can be tested anywhere:
#   pwsh -NoProfile -File scripts/release-helpers.test.ps1

$BetaTagPattern = '^v(\d+)\.(\d+)\.(\d+)-beta\.(\d+)$'

function ConvertTo-BetaVersionObject {
    param([Parameter(Mandatory)][string]$Tag)
    if ($Tag -match $BetaTagPattern) {
        return [pscustomobject]@{
            Tag  = $Tag
            Base = [version]"$($Matches[1]).$($Matches[2]).$($Matches[3])"
            Beta = [int]$Matches[4]
        }
    }
    return $null
}

# Highest beta tag by (base version, beta number). Burned tags (a failed release still occupies its
# tag) are included on purpose, so the suggestion is never an occupied version.
function Get-LatestBetaTag {
    param([string[]]$Tags)
    $parsed = @($Tags | ForEach-Object { ConvertTo-BetaVersionObject $_ } | Where-Object { $_ })
    if (-not $parsed) { return $null }
    return $parsed | Sort-Object -Property @{Expression = { $_.Base }; Descending = $true }, @{Expression = { $_.Beta }; Descending = $true } | Select-Object -First 1
}

function Get-NextBetaVersion {
    param([string[]]$Tags)
    $latest = Get-LatestBetaTag $Tags
    if (-not $latest) { throw "GitHub does not contain a beta version tag (vX.Y.Z-beta.N)." }
    return "$($latest.Base)-beta.$($latest.Beta + 1)"
}

# $true only for X.Y.Z-beta.N that sorts strictly after the latest existing beta tag.
function Assert-BetaVersion {
    param([Parameter(Mandatory)][string]$Version, [string[]]$Tags)
    if ($Version -notmatch '^\d+\.\d+\.\d+-beta\.\d+$') {
        throw "Version '$Version' is not a beta version. Use X.Y.Z-beta.N (the beta asset audit requires it)."
    }
    $candidate = ConvertTo-BetaVersionObject "v$Version"
    $latest = Get-LatestBetaTag $Tags
    if ($latest) {
        $newer = ($candidate.Base -gt $latest.Base) -or (($candidate.Base -eq $latest.Base) -and ($candidate.Beta -gt $latest.Beta))
        if (-not $newer) {
            throw "Version $Version is not newer than the latest beta tag $($latest.Tag)."
        }
    }
}

function Test-ChangelogHasVersion {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Version)
    if (-not (Test-Path $Path)) { return $false }
    $pattern = '(?m)^## \[' + [regex]::Escape($Version) + '\]'
    return [bool](Select-String -Path $Path -Pattern $pattern -Quiet)
}

# Only these origins may release the beta. Never the main product repository.
function Test-BetaOrigin {
    param([string]$Url)
    return [bool]($Url -match '[:/]tage-ilot/stagepilot-beta(\.git)?/?$')
}
