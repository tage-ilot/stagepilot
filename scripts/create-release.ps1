[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [string]$TargetBranch,

    [switch]$Yes
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root
. (Join-Path $PSScriptRoot "release-helpers.ps1")
$Repo = "tage-ilot/stagepilot"
$env:GIT_PAGER = "cat"
$env:PAGER = "cat"

function Invoke-Checked {
    param(
        [Parameter(Mandatory)]
        [string]$Label,
        [Parameter(Mandatory)]
        [scriptblock]$Command
    )

    Write-Host "`n==> $Label" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE."
    }
}

function Require-Command {
    param([Parameter(Mandatory)][string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found. Install it and run the release command again."
    }
}

function Invoke-NpmPackage {
    param(
        [Parameter(Mandatory)][string]$Directory,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    Push-Location (Join-Path $Root $Directory)
    try {
        & npm @Arguments
    }
    finally {
        Pop-Location
    }
}

foreach ($command in @("git", "gh", "node", "npm", "uv", "cargo")) {
    Require-Command $command
}

$GitTopLevel = git rev-parse --show-toplevel 2>$null
if ($LASTEXITCODE -ne 0 -or -not $GitTopLevel) {
    throw "Run this command from the StagePilot repository."
}

# Comparing absolute paths is unreliable for mapped network drives because Git
# canonicalizes them to UNC paths while PowerShell can retain the drive letter.
$GitPrefix = git rev-parse --show-prefix 2>$null
if ($LASTEXITCODE -ne 0 -or $GitPrefix) {
    throw "Run this command from the StagePilot repository."
}

$OriginUrl = (git remote get-url origin 2>$null)
if ($LASTEXITCODE -ne 0 -or -not (Test-ReleaseOrigin $OriginUrl)) {
    throw "origin is '$OriginUrl'. This script releases $Repo only; it will not tag or push anything else."
}

if (-not $env:CARGO_TARGET_DIR -and $GitTopLevel.StartsWith("//")) {
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "StagePilot\cargo-target"
    Write-Host "Using local Cargo cache for NAS checkout: $env:CARGO_TARGET_DIR" -ForegroundColor Cyan
}

Invoke-Checked "Verify GitHub authentication" { gh auth status }

$RemoteTags = @(gh api repos/$Repo/tags --paginate --jq '.[].name')
if ($LASTEXITCODE -ne 0 -or -not $RemoteTags) {
    throw "Could not read the existing StagePilot version tags on GitHub."
}
$Latest = Get-LatestTag $RemoteTags
$Suggested = Get-NextVersion $RemoteTags
$SuggestedTag = "v$Suggested"
Write-Host "`nLatest GitHub release: StagePilot $($Latest.Tag)" -ForegroundColor Cyan

if (-not $Version) {
    $Version = Read-Host "New StagePilot version (default $SuggestedTag)"
    if (-not $Version) {
        $Version = $SuggestedTag
    }
}
$Version = $Version.Trim().TrimStart("v")
Assert-ReleaseVersion $Version $RemoteTags

$Tag = "v$Version"
$CurrentBranch = (git branch --show-current).Trim()
if (-not $CurrentBranch) {
    throw "Releases cannot be created from a detached HEAD."
}
if (git tag --list $Tag) {
    throw "Local tag $Tag already exists."
}
if (git ls-remote --exit-code --tags origin "refs/tags/$Tag" 2>$null) {
    throw "Remote tag $Tag already exists."
}

if (-not $TargetBranch) {
    if ($Yes) {
        $TargetBranch = "main"
    }
    else {
        $destination = Read-Host "Push the release commit to main or another branch? (M/B, default M)"
        if (-not $destination -or $destination -match '^(?i:m|main)$') {
            $TargetBranch = "main"
        }
        elseif ($destination -match '^(?i:b|branch)$') {
            $TargetBranch = (Read-Host "Branch name").Trim()
        }
        else {
            throw "Choose M for main or B for another branch."
        }
    }
}
$TargetBranch = $TargetBranch.Trim()
if (-not $TargetBranch) {
    throw "The target branch cannot be empty."
}
Invoke-Checked "Validate target branch name" {
    git check-ref-format --branch $TargetBranch
}

if ($TargetBranch -eq "main") {
    Invoke-Checked "Fetch origin/main" { git fetch origin main }
    git merge-base --is-ancestor origin/main HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "The current checkout does not contain the latest origin/main. Reconcile it before releasing to main."
    }
}

if (-not (Test-ChangelogHasVersion (Join-Path $Root "CHANGELOG.md") $Version)) {
    throw "CHANGELOG.md has no '## [$Version]' heading. Add the release notes under it, commit them, then run this again (nothing was changed)."
}
$Dirty = git status --porcelain
if ($Dirty) {
    Write-Host "`nNote: uncommitted changes exist and WILL be included in the release commit." -ForegroundColor Yellow
}

Write-Host "`nRelease plan" -ForegroundColor Yellow
Write-Host "  Title:       StagePilot $Tag"
if (Test-PreReleaseVersion $Version) {
    Write-Host "  Type:        RELEASE CANDIDATE - Rust (in-process) installers, GitHub pre-release, never offered to installed apps" -ForegroundColor Yellow
}
Write-Host "  Source:      $CurrentBranch"
Write-Host "  Destination: origin/$TargetBranch"
Write-Host "`nCurrent working changes:" -ForegroundColor Yellow
git --no-pager status --short

if (-not $Yes) {
    $confirmation = Read-Host "Type YES to begin the unattended release"
    if ($confirmation.Trim().ToUpperInvariant() -ne "YES") {
        throw "Release cancelled. No files, commit, tag, or remote branch were changed."
    }
}

Invoke-Checked "Update all StagePilot version sources" {
    node scripts/set-release-version.mjs $Version
}
Invoke-Checked "Refresh the Python lockfile" { uv lock --project backend }
Invoke-Checked "Refresh the frontend lockfile" {
    Invoke-NpmPackage "frontend" @("install", "--package-lock-only", "--ignore-scripts")
}
Invoke-Checked "Refresh the desktop lockfile" {
    Invoke-NpmPackage "desktop" @("install", "--package-lock-only", "--ignore-scripts")
}
Invoke-Checked "Validate version and updater configuration" {
    node scripts/validate_versions.mjs $Tag
}

Invoke-Checked "Install locked backend dependencies" {
    uv sync --project backend --extra dev --locked
}
Invoke-Checked "Format backend source" {
    uv run --project backend ruff format backend/src backend/tests
}
Invoke-Checked "Install locked frontend dependencies" {
    Invoke-NpmPackage "frontend" @("ci")
}
Invoke-Checked "Install locked desktop dependencies" {
    Invoke-NpmPackage "desktop" @("ci")
}
Invoke-Checked "Format Rust source" {
    cargo fmt --manifest-path desktop/src-tauri/Cargo.toml
}

# The same command list the release workflows and the release-validate.yml dry run use
# (scripts/validate_application.sh), so a local pass means the tag's validation passes.
# Git for Windows ships bash next to git; prefer it over the WSL bash.exe launcher.
$Bash = $null
$GitCommand = Get-Command git -ErrorAction SilentlyContinue
if ($GitCommand) {
    $GitBash = Join-Path (Split-Path (Split-Path $GitCommand.Source)) "bin/bash.exe"
    if (Test-Path $GitBash) { $Bash = $GitBash }
}
if (-not $Bash) {
    $Found = Get-Command bash -ErrorAction SilentlyContinue
    if (-not $Found) { throw "bash was not found; install Git for Windows (it ships bash)." }
    $Bash = $Found.Source
}
# Every release is the Rust in-process app (stable and pre-release alike).
$ValidationVariant = "rust"
Invoke-Checked "Validate application ($ValidationVariant variant)" {
    & $Bash scripts/validate_application.sh $ValidationVariant
}

if (Test-Path "tools/multitracks-cues/package.json") {
    Invoke-Checked "Install locked MultiTracks CLI dependencies" {
        Invoke-NpmPackage "tools/multitracks-cues" @("ci")
    }
    Invoke-Checked "MultiTracks CLI typecheck" {
        Invoke-NpmPackage "tools/multitracks-cues" @("run", "typecheck")
    }
    Invoke-Checked "MultiTracks CLI tests" {
        Invoke-NpmPackage "tools/multitracks-cues" @("test", "--", "--run")
    }
    Invoke-Checked "MultiTracks CLI lint" {
        Invoke-NpmPackage "tools/multitracks-cues" @("run", "lint")
    }
    Invoke-Checked "MultiTracks CLI build" {
        Invoke-NpmPackage "tools/multitracks-cues" @("run", "build")
    }
}

Invoke-Checked "Release workflow runner boundary" { python scripts/validate_workflow_runners.py }
Invoke-Checked "Source export audit" { node scripts/audit_release.mjs source }

Invoke-Checked "Check the release diff" { git --no-pager diff --check }

Write-Host "`nFiles that will be included:" -ForegroundColor Yellow
git --no-pager diff --stat
git --no-pager status --short

Invoke-Checked "Stage all source changes" { git add --all }

$forbidden = git diff --cached --name-only |
    Where-Object { $_ -match '(^|/)(node_modules|dist|target|build|graphify-out|\.codex)(/|$)' -or $_ -match '\.(key|pem)$' }
if ($forbidden) {
    git reset
    throw "Release refused because generated or sensitive files were staged:`n$($forbidden -join "`n")"
}
if (-not (git diff --cached --name-only)) {
    throw "There are no changes to release."
}
Invoke-Checked "Check the staged release diff" { git --no-pager diff --cached --check }

Invoke-Checked "Commit StagePilot $Tag" {
    git commit -m "Release StagePilot $Tag" -m "Include the validated backend, frontend, desktop, documentation, and tooling changes."
}
Invoke-Checked "Push release commit to origin/$TargetBranch" {
    git push origin "HEAD:refs/heads/$TargetBranch"
}
Invoke-Checked "Create release tag $Tag" { git tag -a $Tag -m "StagePilot $Tag" }
Invoke-Checked "Push release tag $Tag" { git push origin $Tag }

if (Test-PreReleaseVersion $Version) {
    Write-Host "`nThis is a release candidate: it will be published as a GitHub pre-release and will NOT become Latest or reach the in-app updater." -ForegroundColor Yellow
}
Write-Host "`nTag $Tag is pushed. GitHub builds the Windows, Intel Mac and Apple Silicon packages (about 20-40 minutes)." -ForegroundColor Cyan
$RunId = $null
for ($attempt = 0; $attempt -lt 12 -and -not $RunId; $attempt += 1) {
    Start-Sleep -Seconds 5
    $RunId = gh run list --repo $Repo --workflow release-macos.yml --event push --branch $Tag --limit 1 --json databaseId --jq '.[0].databaseId'
}
if (-not $RunId) {
    throw "The tag was pushed, but the GitHub release workflow was not found. Check https://github.com/$Repo/actions manually."
}
Write-Host "Release run: https://github.com/$Repo/actions/runs/$RunId"
Write-Host "A background watcher on the Hermes host reports the result to WhatsApp; you do not need to keep this window open." -ForegroundColor Cyan

if ($env:STAGEPILOT_RELEASE_WATCH -eq "1") {
    Invoke-Checked "Build and publish StagePilot $Tag on GitHub" {
        gh run watch $RunId --repo $Repo --interval 30 --exit-status
    }
    $ReleaseUrl = gh release view $Tag --repo $Repo --json url --jq '.url'
    Write-Host "`nStagePilot $Tag is published:" -ForegroundColor Green
    Write-Host $ReleaseUrl
}
