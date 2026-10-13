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

$tags = @("v1.1.103", "v1.1.103-beta.6", "v1.1.104-beta.9", "v1.1.104-beta.31", "v1.1.104-beta.32", "v1.1.104-beta.33", "v1.1.104-beta.4", "not-a-tag")

Check "next beta is one past the highest, numerically (33 beats 4 and 9)" { Expect (Get-NextBetaVersion $tags) "1.1.104-beta.34" }
Check "a higher base wins over a higher beta number" { Expect (Get-NextBetaVersion @("v1.1.104-beta.40", "v1.1.105-beta.1")) "1.1.105-beta.2" }
Check "stable tags are ignored" { Expect (Get-NextBetaVersion @("v1.1.200", "v1.1.104-beta.2")) "1.1.104-beta.3" }
Check "no beta tag at all is an error" { ExpectThrow { Get-NextBetaVersion @("v1.1.103") } "*does not contain a beta*" }
Check "accepts the suggested version" { Assert-BetaVersion "1.1.104-beta.34" $tags }
Check "accepts a new base" { Assert-BetaVersion "1.1.105-beta.1" $tags }
Check "rejects an existing tag" { ExpectThrow { Assert-BetaVersion "1.1.104-beta.33" $tags } "*not newer*" }
Check "rejects a lower number" { ExpectThrow { Assert-BetaVersion "1.1.104-beta.5" $tags } "*not newer*" }
Check "rejects a stable version" { ExpectThrow { Assert-BetaVersion "1.1.104" $tags } "*not a beta*" }
Check "rejects rc/alpha" { ExpectThrow { Assert-BetaVersion "1.1.104-rc.1" $tags } "*not a beta*" }
Check "origin: beta repo https" { Expect (Test-BetaOrigin "https://github.com/tage-ilot/stagepilot-beta.git") $true }
Check "origin: beta repo ssh" { Expect (Test-BetaOrigin "git@github.com:tage-ilot/stagepilot-beta.git") $true }
Check "origin: beta repo no suffix" { Expect (Test-BetaOrigin "https://github.com/tage-ilot/stagepilot-beta") $true }
Check "origin: the MAIN repo is refused" { Expect (Test-BetaOrigin "https://github.com/tage-ilot/stagepilot.git") $false }
Check "origin: a lookalike is refused" { Expect (Test-BetaOrigin "https://github.com/other/tage-ilot/stagepilot-beta-fork") $false }

$cl = Join-Path ([IO.Path]::GetTempPath()) ("cl-" + [guid]::NewGuid() + ".md")
Set-Content $cl "# Changelog`n`n## [Unreleased]`n`n## [1.1.104-beta.33] - 2026-10-12`n"
Check "changelog finds an existing heading" { Expect (Test-ChangelogHasVersion $cl "1.1.104-beta.33") $true }
Check "changelog missing heading" { Expect (Test-ChangelogHasVersion $cl "1.1.104-beta.34") $false }
Check "changelog heading is matched exactly (3 vs 33)" { Expect (Test-ChangelogHasVersion $cl "1.1.104-beta.3") $false }
Remove-Item $cl -Force

if ($script:failed) { Write-Host "`n$script:failed check(s) failed"; exit 1 }
Write-Host "`nall checks passed"
