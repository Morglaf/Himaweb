# Build and run HimaWeb (localhost:8787 only)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
  $Root = $PSScriptRoot
  if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    $Root = Get-Location
  }
}
Set-Location $Root

# Stop any running himaweb instances (free port + unlock binary/incremental cache)
$running = Get-Process -Name "himaweb" -ErrorAction SilentlyContinue
if ($running) {
  Write-Host "Arrêt de $($running.Count) instance(s) himaweb..." -ForegroundColor Yellow
  $running | Stop-Process -Force -ErrorAction SilentlyContinue
  $deadline = (Get-Date).AddSeconds(5)
  while ((Get-Process -Name "himaweb" -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 200
  }
  Start-Sleep -Milliseconds 400
}

function Test-Bin([string]$Name, [string]$EnvVar) {
  $override = [Environment]::GetEnvironmentVariable($EnvVar)
  if ($override) {
    if (Test-Path $override) { return $true }
    Write-Host "WARN: $EnvVar=$override introuvable" -ForegroundColor Yellow
    return $false
  }
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  return [bool]$cmd
}

Write-Host "=== HimaWeb — vérification des binaires ===" -ForegroundColor Cyan
$him = Test-Bin "himalaya" "HIMAWEB_HIMALAYA_BIN"
$card = Test-Bin "cardamum" "HIMAWEB_CARDAMUM_BIN"
$cal = Test-Bin "calendula" "HIMAWEB_CALENDULA_BIN"

if ($him) { Write-Host "[OK] himalaya" -ForegroundColor Green }
else {
  Write-Host "[!] himalaya ABSENT — mode hors-ligne / page d'état" -ForegroundColor Yellow
  Write-Host "    Installez: cargo install himalaya   ou via winget si disponible" -ForegroundColor DarkYellow
  Write-Host "    Override:  `$env:HIMAWEB_HIMALAYA_BIN = 'C:\path\himalaya.exe'" -ForegroundColor DarkYellow
}
if ($card) { Write-Host "[OK] cardamum" -ForegroundColor Green }
else { Write-Host "[i] cardamum absent — autocomplete contacts désactivé" -ForegroundColor DarkGray }
if ($cal) { Write-Host "[OK] calendula" -ForegroundColor Green }
else { Write-Host "[i] calendula absent — calendrier dégradé" -ForegroundColor DarkGray }

function Invoke-CargoBuild([string[]]$BuildArgs) {
  $maxAttempts = 3
  for ($i = 1; $i -le $maxAttempts; $i++) {
    $output = & cargo @BuildArgs 2>&1
    $output | Write-Host
    if ($LASTEXITCODE -eq 0) { return $true }
    $text = ($output | Out-String)
    if ($i -lt $maxAttempts -and $text -match "os error 32|utilisé par un autre processus|being used by another process") {
      Write-Host "Cache incremental verrouillé — nouvelle tentative ($i/$maxAttempts)..." -ForegroundColor Yellow
      Get-Process -Name "himaweb" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
      Start-Sleep -Seconds 1
      continue
    }
    return $false
  }
  return $false
}

$Release = $args -contains "--release"
if ($Release) {
  Write-Host "`nCompilation release..." -ForegroundColor Cyan
  if (-not (Invoke-CargoBuild @("build", "--release"))) {
    throw "Échec compilation release (code $LASTEXITCODE). Fermez les autres instances himaweb/cargo puis relancez."
  }
  $bin = Join-Path $Root "target\release\himaweb.exe"
} else {
  Write-Host "`nCompilation debug..." -ForegroundColor Cyan
  if (-not (Invoke-CargoBuild @("build"))) {
    throw "Échec compilation debug (code $LASTEXITCODE). Fermez les autres instances himaweb/cargo puis relancez."
  }
  $bin = Join-Path $Root "target\debug\himaweb.exe"
}

if (-not (Test-Path $bin)) {
  throw "Binaire introuvable: $bin"
}

Write-Host "`nDémarrage http://127.0.0.1:8787 ..." -ForegroundColor Cyan
& $bin
