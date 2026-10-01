# Publie une nouvelle release HimaWeb (bump version -> commit -> tag -> push).
# La CI GitHub Actions construit les binaires et les attache a la release.
#
# Usage :
#   .\scripts\release.ps1                      # patch : 0.1.0 -> 0.1.1
#   .\scripts\release.ps1 -Bump minor          # 0.1.0 -> 0.2.0
#   .\scripts\release.ps1 -Bump major          # 0.1.0 -> 1.0.0
#   .\scripts\release.ps1 -Version 0.3.0       # version exacte
#   .\scripts\release.ps1 -IncludeChanges      # inclut aussi les fichiers modifies non commites
#   .\scripts\release.ps1 -DryRun              # affiche ce qui serait fait, sans ecrire
#   .\scripts\release.ps1 -Wait                # attend la fin du workflow Release

[CmdletBinding()]
param(
    [ValidateSet("patch", "minor", "major")]
    [string]$Bump = "patch",
    [string]$Version,
    [string]$Message,
    [switch]$IncludeChanges,
    [switch]$DryRun,
    [switch]$Wait
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    $Root = (Get-Location).Path
}
Set-Location $Root

function Get-CargoVersion {
    $toml = Get-Content -Raw (Join-Path $Root "Cargo.toml")
    if ($toml -match '(?m)^version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    throw "version introuvable dans Cargo.toml"
}

function Set-CargoVersion([string]$NewVersion) {
    $path = Join-Path $Root "Cargo.toml"
    $toml = Get-Content -Raw $path
    $updated = [regex]::Replace(
        $toml,
        '(?m)^version\s*=\s*"[^"]+"',
        "version = `"$NewVersion`"",
        1
    )
    if ($updated -eq $toml) {
        throw "impossible de mettre a jour version dans Cargo.toml"
    }
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($path, $updated, $utf8)
}

function Bump-SemVer([string]$Current, [string]$Kind) {
    $parts = $Current.Split('.')
    if ($parts.Count -lt 3) { throw "version semver attendue (x.y.z), recu: $Current" }
    $major = [int]$parts[0]
    $minor = [int]$parts[1]
    $patch = [int]$parts[2]
    switch ($Kind) {
        "major" { $major++; $minor = 0; $patch = 0 }
        "minor" { $minor++; $patch = 0 }
        "patch" { $patch++ }
    }
    return "$major.$minor.$patch"
}

function Assert-GitOk {
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    if ($branch -ne "master" -and $branch -ne "main") {
        throw "Branche actuelle: $branch - basculez sur master/main avant de releaser."
    }

    $remote = (git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>$null)
    if (-not $remote) {
        throw "Aucune branche amont (upstream). Faites: git push -u origin HEAD"
    }

    git fetch origin --tags --quiet 2>$null | Out-Null

    $status = git status --porcelain
    if ($status -and -not $IncludeChanges) {
        Write-Host "Arbre git non propre :" -ForegroundColor Yellow
        git status --short
        if ($DryRun) {
            Write-Host "(DryRun) en release reelle: commitez, ou passez -IncludeChanges." -ForegroundColor DarkYellow
            return
        }
        throw "Commitez (ou stash) d'abord, ou relancez avec -IncludeChanges."
    }
}

$current = Get-CargoVersion
if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        throw "-Version doit etre du type 1.2.3 (sans prefixe v)"
    }
    $newVersion = $Version
} else {
    $newVersion = Bump-SemVer $current $Bump
}

$tag = "v$newVersion"
$commitMsg = if ($Message) { $Message } else { "release: $tag" }

Write-Host "=== HimaWeb release ===" -ForegroundColor Cyan
Write-Host "Version : $current -> $newVersion"
Write-Host "Tag     : $tag"
Write-Host "Commit  : $commitMsg"
if ($IncludeChanges) { Write-Host "Mode    : Inclure les changements locaux" -ForegroundColor DarkYellow }
if ($DryRun) { Write-Host "Dry-run : aucune ecriture / push" -ForegroundColor DarkYellow }

Assert-GitOk

$existing = git rev-parse -q --verify "refs/tags/$tag" 2>$null
if ($existing) {
    throw "Le tag $tag existe deja localement."
}
$remoteTag = git ls-remote --tags origin "refs/tags/$tag" 2>$null
if ($remoteTag) {
    throw "Le tag $tag existe deja sur origin."
}

if ($DryRun) {
    Write-Host ""
    Write-Host "[DryRun] OK - aurait mis a jour Cargo.toml, commit, tag $tag, push branch + tag." -ForegroundColor Green
    exit 0
}

Write-Host ""
Write-Host "1/5 Cargo.toml -> $newVersion" -ForegroundColor Cyan
Set-CargoVersion $newVersion
Write-Host "    refresh Cargo.lock..."
cargo check --quiet 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host "    cargo check a signale une erreur - verifiez avant d'installer la release." -ForegroundColor Yellow
}

Write-Host "2/5 git add / commit" -ForegroundColor Cyan
if ($IncludeChanges) {
    git add -A
} else {
    git add Cargo.toml Cargo.lock
}
$staged = git diff --cached --name-only
if (-not $staged) {
    throw "Rien a committer (version deja a $newVersion ?)."
}
git commit -m $commitMsg
if ($LASTEXITCODE -ne 0) { throw "git commit a echoue" }

Write-Host "3/5 tag $tag" -ForegroundColor Cyan
git tag -a $tag -m $commitMsg
if ($LASTEXITCODE -ne 0) { throw "git tag a echoue" }

Write-Host "4/5 push branch + tag" -ForegroundColor Cyan
git push origin HEAD
if ($LASTEXITCODE -ne 0) { throw "git push branch a echoue" }
git push origin $tag
if ($LASTEXITCODE -ne 0) { throw "git push tag a echoue" }

Write-Host "5/5 workflow Release declenche" -ForegroundColor Cyan
$repo = (gh repo view --json nameWithOwner -q .nameWithOwner 2>$null)
if (-not $repo) { $repo = "Morglaf/Himaweb" }
$actionsUrl = "https://github.com/$repo/actions/workflows/release.yml"
$releaseUrl = "https://github.com/$repo/releases/tag/$tag"

Write-Host ""
Write-Host "Release $tag poussee." -ForegroundColor Green
Write-Host "CI     : $actionsUrl"
Write-Host "Page   : $releaseUrl"
Write-Host "Quand la CI est verte, l'install recupere automatiquement latest :"
Write-Host "  irm https://raw.githubusercontent.com/$repo/master/install.ps1 | iex" -ForegroundColor DarkGray
Write-Host "  curl -sSL https://raw.githubusercontent.com/$repo/master/install.sh | PREFIX=~/.local sh" -ForegroundColor DarkGray

if ($Wait) {
    Write-Host ""
    Write-Host "Attente du workflow Release..." -ForegroundColor Cyan
    Start-Sleep -Seconds 8
    $runId = gh run list --workflow=release.yml --branch $tag --limit 1 --json databaseId -q '.[0].databaseId' 2>$null
    if ($runId) {
        gh run watch $runId --exit-status
        if ($LASTEXITCODE -eq 0) {
            Write-Host "CI OK - assets disponibles sur $releaseUrl" -ForegroundColor Green
        } else {
            Write-Host "CI en echec - voir $actionsUrl" -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "Run introuvable pour l'instant - surveillez $actionsUrl" -ForegroundColor Yellow
    }
}
