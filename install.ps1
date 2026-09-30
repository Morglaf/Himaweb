# Installe le dernier binaire HimaWeb depuis GitHub Releases (Windows x64).
# Usage (PowerShell) :
#   irm https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.ps1 | iex
#   .\install.ps1 -Prefix "$env:LOCALAPPDATA\HimaWeb"
#
# Par défaut : %LOCALAPPDATA%\HimaWeb\bin (ajouté au PATH utilisateur si possible).

[CmdletBinding()]
param(
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA "HimaWeb"),
    [string]$Tag = "latest"
)

$ErrorActionPreference = "Stop"

$releasesBase = "https://github.com/Morglaf/Himaweb/releases"
$assetName = "himaweb.x86_64-windows.zip"
$binDir = Join-Path $Prefix "bin"
$exePath = Join-Path $binDir "himaweb.exe"

if ($Tag -eq "latest") {
    $url = "$releasesBase/latest/download/$assetName"
} else {
    $url = "$releasesBase/download/$Tag/$assetName"
}

$tmpdir = Join-Path ([System.IO.Path]::GetTempPath()) ("himaweb-install-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $tmpdir | Out-Null
try {
    $zipPath = Join-Path $tmpdir $assetName
    Write-Host "Téléchargement de $url …"
    Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing

    Write-Host "Extraction…"
    Expand-Archive -Path $zipPath -DestinationPath $tmpdir -Force

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    $src = Get-ChildItem -Path $tmpdir -Filter "himaweb.exe" -Recurse | Select-Object -First 1
    if (-not $src) {
        throw "himaweb.exe introuvable dans l’archive."
    }
    Copy-Item -Path $src.FullName -Destination $exePath -Force

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$binDir*") {
        [Environment]::SetEnvironmentVariable("Path", ($userPath.TrimEnd(';') + ";" + $binDir), "User")
        $env:Path = $env:Path.TrimEnd(';') + ";" + $binDir
        Write-Host "Ajouté au PATH utilisateur : $binDir"
        Write-Host "(Ouvrez un nouveau terminal pour que le PATH soit pris en compte partout.)"
    }

    Write-Host "HimaWeb installé : $exePath"
    & $exePath --version
    Write-Host "Lancez avec : himaweb"
} finally {
    Remove-Item -Recurse -Force $tmpdir -ErrorAction SilentlyContinue
}
