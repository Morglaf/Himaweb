# Compile HimaWeb-Setup-x64.exe (Inno Setup 6).
# Prérequis : ISCC.exe dans le PATH ou installation standard Inno Setup 6.
#
# Usage :
#   .\scripts\build-installer.ps1
#   .\scripts\build-installer.ps1 -Version 0.1.4 -ExePath .\target\release\himaweb.exe

[CmdletBinding()]
param(
    [string]$Version,
    [string]$ExePath,
    [string]$OutDir
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $Version) {
    $toml = Get-Content -Raw (Join-Path $Root "Cargo.toml")
    if ($toml -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    }
    else {
        throw "version introuvable dans Cargo.toml"
    }
}

if (-not $ExePath) {
    $ExePath = Join-Path $Root "target\release\himaweb.exe"
    if (-not (Test-Path $ExePath)) {
        $ExePath = Join-Path $Root "target\x86_64-pc-windows-msvc\release\himaweb.exe"
    }
}
if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "himaweb.exe introuvable ($ExePath). Lancez d’abord : cargo build --release"
}

$iscc = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $iscc) {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c) {
            $iscc = @{ Source = $c }
            break
        }
    }
}
if (-not $iscc) {
    throw "Inno Setup 6 (ISCC.exe) introuvable. Installez-le ou ajoutez iscc au PATH."
}

$iss = Join-Path $Root "installer\himaweb.iss"
$exeAbs = (Resolve-Path -LiteralPath $ExePath).Path
$isccPath = if ($iscc.Source) { $iscc.Source } else { $iscc.Path }

Write-Host "ISCC : $isccPath"
Write-Host "Version : $Version"
Write-Host "Exe : $exeAbs"

& $isccPath `
    "/DMyAppVersion=$Version" `
    "/DMyAppExeSource=$exeAbs" `
    $iss

$setup = Join-Path $Root "dist\HimaWeb-Setup-x64.exe"
if (-not (Test-Path -LiteralPath $setup)) {
    throw "Setup non produit : $setup"
}
Write-Host "OK : $setup"
if ($OutDir) {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    Copy-Item $setup (Join-Path $OutDir "HimaWeb-Setup-x64.exe") -Force
}
