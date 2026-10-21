# Plain-PowerShell tests (no Pester needed): pwsh -NoProfile -File scripts/release-helpers.test.ps1
$ErrorActionPreference = "Stop"
. "$PSScriptRoot/release-helpers.ps1"
$script:failed = 0
function Check { param([string]$Name, [scriptblock]$Body)
    try { & $Body; Write-Host "ok   $Name" } catch { $script:failed += 1; Write-Host "FAIL $Name :: $($_.Exception.Message)" }
}
function Expect { param($Actual, $Expected) if ($Actual -ne $Expected) { throw "expected '$Expected' got '$Actual'" } }
function ExpectThrow { param([scriptblock]$Body, [string]$Like)
    try { & $Body } catch { if ($_.Exception.Message -notlike $Like) { throw "wrong message: $($_.Exception.Message)" }; return }
    throw "did not throw"
}

$tags = @("v1.1.103", "v1.1.104", "v2.41.0", "v2.9.0", "v2.41.10", "v1.1.104-beta.41", "not-a-tag")

Check "next is one patch past the highest, numerically (2.41.10 beats 2.9.0)" { Expect (Get-NextVersion $tags) "2.41.11" }
Check "a higher minor wins over a higher patch" { Expect (Get-NextVersion @("v2.40.99", "v2.41.0")) "2.41.1" }
Check "beta tags are ignored" { Expect (Get-NextVersion @("v2.41.0", "v9.9.9-beta.1")) "2.41.1" }
Check "no version tag at all is an error" { ExpectThrow { Get-NextVersion @("v1.1.104-beta.2") } "*does not contain a version*" }
Check "accepts the suggested version" { Assert-ReleaseVersion "2.41.11" $tags }
Check "accepts a new minor" { Assert-ReleaseVersion "2.42.0" $tags }
Check "rejects an existing tag" { ExpectThrow { Assert-ReleaseVersion "2.41.10" $tags } "*not newer*" }
Check "rejects a lower version" { ExpectThrow { Assert-ReleaseVersion "2.41.5" $tags } "*not newer*" }
Check "rejects a two-part version" { ExpectThrow { Assert-ReleaseVersion "2.42" $tags } "*not valid*" }
Check "rejects a beta version" { ExpectThrow { Assert-ReleaseVersion "2.42.0-beta.1" $tags } "*not valid*" }
Check "rejects an alpha version" { ExpectThrow { Assert-ReleaseVersion "2.42.0-alpha.1" $tags } "*not valid*" }
Check "rejects an rc without a number" { ExpectThrow { Assert-ReleaseVersion "2.42.0-rc" $tags } "*not valid*" }
Check "accepts the first rc of a new minor" { Assert-ReleaseVersion "2.42.0-rc.1" $tags }
Check "accepts an rc of the next patch" { Assert-ReleaseVersion "2.41.11-rc.1" $tags }
Check "rejects an rc of an already released version" { ExpectThrow { Assert-ReleaseVersion "2.41.10-rc.1" $tags } "*not newer than the latest stable*" }
Check "rejects an rc of an older version" { ExpectThrow { Assert-ReleaseVersion "2.40.0-rc.1" $tags } "*not newer than the latest stable*" }
$rcTags = $tags + @("v2.42.0-rc.1", "v2.42.0-rc.2")
Check "rc.3 follows existing rc.1 and rc.2" { Assert-ReleaseVersion "2.42.0-rc.3" $rcTags }
Check "rejects a repeated rc number" { ExpectThrow { Assert-ReleaseVersion "2.42.0-rc.2" $rcTags } "*not newer than the existing rc*" }
Check "rejects a lower rc number" { ExpectThrow { Assert-ReleaseVersion "2.42.0-rc.1" $rcTags } "*not newer than the existing rc*" }
Check "the stable release after its rcs is accepted" { Assert-ReleaseVersion "2.42.0" $rcTags }
Check "rc tags do not move the suggested stable version" { Expect (Get-NextVersion $rcTags) "2.41.11" }
Check "Test-PreReleaseVersion: rc" { Expect (Test-PreReleaseVersion "2.42.0-rc.1") $true }
Check "Test-PreReleaseVersion: stable" { Expect (Test-PreReleaseVersion "2.42.0") $false }
Check "origin: repo https" { Expect (Test-ReleaseOrigin "https://github.com/tage-ilot/stagepilot.git") $true }
Check "origin: repo ssh" { Expect (Test-ReleaseOrigin "git@github.com:tage-ilot/stagepilot.git") $true }
Check "origin: repo no suffix" { Expect (Test-ReleaseOrigin "https://github.com/tage-ilot/stagepilot") $true }
Check "origin: the archived alpha repo is refused" { Expect (Test-ReleaseOrigin "https://github.com/tage-ilot/stagepilot-alpha.git") $false }
Check "origin: a lookalike is refused" { Expect (Test-ReleaseOrigin "https://github.com/other/tage-ilot/stagepilot-fork") $false }

$cl = Join-Path ([IO.Path]::GetTempPath()) ("cl-" + [guid]::NewGuid() + ".md")
Set-Content $cl "# Changelog`n`n## [Unreleased]`n`n## [2.41.0] - 2026-10-16`n"
Check "changelog finds an existing heading" { Expect (Test-ChangelogHasVersion $cl "2.41.0") $true }
Check "changelog missing heading" { Expect (Test-ChangelogHasVersion $cl "2.41.1") $false }
Check "changelog heading is matched exactly (2.41.0 vs 2.41.00)" { Expect (Test-ChangelogHasVersion $cl "2.41.00") $false }
Remove-Item $cl -Force

if ($script:failed) { Write-Host "`n$script:failed check(s) failed"; exit 1 }
Write-Host "`nall checks passed"
