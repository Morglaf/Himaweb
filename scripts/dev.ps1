# Build and run HimaWeb (localhost:8787 only)
# himaweb est lancé DETACHÉ du terminal Cursor (CREATE_BREAKAWAY_FROM_JOB) :
# fermer l’onglet ne le tue plus. Relancer ce script remplace l’instance.

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
  $Root = $PSScriptRoot
  if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    $Root = Get-Location
  }
}
Set-Location $Root

function Stop-HimaWebListeners {
  Write-Host "Nettoyage himaweb / port 8787..." -ForegroundColor Yellow

  $running = @(Get-Process -Name "himaweb" -ErrorAction SilentlyContinue)
  if ($running.Count -gt 0) {
    Write-Host "  → arrêt de $($running.Count) process (PID $($running.Id -join ', '))" -ForegroundColor Yellow
    $running | Stop-Process -Force -ErrorAction SilentlyContinue
  }
  & taskkill.exe /F /IM himaweb.exe 2>$null | Out-Null

  $ownerIds = @()
  try {
    $ownerIds = @(
      Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty OwningProcess -Unique
    )
  } catch { }

  if ($ownerIds.Count -eq 0) {
    $net = netstat -ano 2>$null | Select-String ':8787\s+.*LISTENING\s+(\d+)$'
    foreach ($m in $net) {
      if ($m.Matches.Count -gt 0) {
        $ownerIds += [int]$m.Matches[0].Groups[1].Value
      }
    }
    $ownerIds = @($ownerIds | Select-Object -Unique)
  }

  foreach ($ownerPid in $ownerIds) {
    if (-not $ownerPid -or $ownerPid -le 0) { continue }
    $p = Get-Process -Id $ownerPid -ErrorAction SilentlyContinue
    $name = if ($p) { $p.ProcessName } else { '?' }
    Write-Host "  → port 8787 : kill PID $ownerPid ($name)" -ForegroundColor Yellow
    Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue
    & taskkill.exe /F /PID $ownerPid 2>$null | Out-Null
  }

  $deadline = (Get-Date).AddSeconds(5)
  while ((Get-Date) -lt $deadline) {
    $still = Get-Process -Name "himaweb" -ErrorAction SilentlyContinue
    $portBusy = $false
    try {
      $portBusy = [bool](Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue)
    } catch { }
    if (-not $still -and -not $portBusy) { break }
    Start-Sleep -Milliseconds 200
  }
  Start-Sleep -Milliseconds 200
}

function Start-HimaWebDetached([string]$Exe, [string]$WorkDir) {
  # Cursor/VS Code place le shell dans un Job Object qui tue les enfants à la fermeture.
  # CREATE_BREAKAWAY_FROM_JOB + DETACHED_PROCESS sort himaweb de ce job.
  Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class HimaWebDetach {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct STARTUPINFO {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct PROCESS_INFORMATION {
    public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId;
  }
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern bool CreateProcess(
    string lpApplicationName, string lpCommandLine, IntPtr lpProcessAttributes,
    IntPtr lpThreadAttributes, bool bInheritHandles, uint dwCreationFlags,
    IntPtr lpEnvironment, string lpCurrentDirectory,
    ref STARTUPINFO lpStartupInfo, out PROCESS_INFORMATION lpProcessInformation);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool CloseHandle(IntPtr h);
  public const uint DETACHED_PROCESS = 0x00000008;
  public const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
  public const uint CREATE_BREAKAWAY_FROM_JOB = 0x01000000;
  public static int Start(string exe, string cwd) {
    var si = new STARTUPINFO();
    si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
    PROCESS_INFORMATION pi;
    uint flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB;
    string cmd = "\"" + exe + "\"";
    if (!CreateProcess(null, cmd, IntPtr.Zero, IntPtr.Zero, false, flags, IntPtr.Zero, cwd, ref si, out pi))
      throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return pi.dwProcessId;
  }
}
"@ -ErrorAction Stop
  return [HimaWebDetach]::Start($Exe, $WorkDir)
}

Stop-HimaWebListeners

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
      Stop-HimaWebListeners
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
    throw "Échec compilation release (code $LASTEXITCODE)."
  }
  $bin = Join-Path $Root "target\release\himaweb.exe"
} else {
  Write-Host "`nCompilation debug..." -ForegroundColor Cyan
  if (-not (Invoke-CargoBuild @("build"))) {
    throw "Échec compilation debug (code $LASTEXITCODE)."
  }
  $bin = Join-Path $Root "target\debug\himaweb.exe"
}

if (-not (Test-Path $bin)) {
  throw "Binaire introuvable: $bin"
}

Write-Host "`nDémarrage http://127.0.0.1:8787 (détaché du terminal)..." -ForegroundColor Cyan
Stop-HimaWebListeners
$ownerPid = Start-HimaWebDetached -Exe $bin -WorkDir $Root
Write-Host "himaweb PID $ownerPid — fermer ce terminal ne l'arrête plus. Relancer dev.ps1 pour remplacer." -ForegroundColor DarkGray
