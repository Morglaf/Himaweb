# Installe / met à jour les CLI Pimalaya sous <Prefix>\tools\<outil>\ + PATH utilisateur.
# Si déjà installé : compare la version locale à la dernière release GitHub ;
# n’écrase que si une mise à jour est nécessaire (sauf -Force).
#
# Usage :
#   .\install-deps.ps1 -Prefix "$env:LOCALAPPDATA\HimaWeb" -Tools himalaya,cardamum,calendula,ortie
#   .\install-deps.ps1 -Prefix ... -Tools neverest -Ollama
#   .\install-deps.ps1 ... -Force   # réinstalle même si déjà à jour

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Prefix,

    [string[]]$Tools = @(),

    [switch]$Ollama,

    # Conservé pour compat Inno : équivalent à la logique « skip si à jour »
    [switch]$SkipIfPresent,

    [switch]$Force
)

$ErrorActionPreference = "Stop"

$Catalog = @{
    himalaya  = @{ Repo = "pimalaya/himalaya";  Asset = "himalaya.x86_64-windows";  Exe = "himalaya.exe" }
    cardamum  = @{ Repo = "pimalaya/cardamum";  Asset = "cardamum.x86_64-windows";  Exe = "cardamum.exe" }
    calendula = @{ Repo = "pimalaya/calendula"; Asset = "calendula.x86_64-windows"; Exe = "calendula.exe" }
    ortie     = @{ Repo = "pimalaya/ortie";     Asset = "ortie.x86_64-windows";     Exe = "ortie.exe" }
    neverest  = @{ Repo = "pimalaya/neverest";  Asset = "neverest.x86_64-windows";  Exe = "neverest.exe" }
    mirador   = @{ Repo = "pimalaya/mirador";   Asset = "carillon.x86_64-windows";  Exe = "mirador.exe"; FindExe = @("mirador.exe", "carillon.exe") }
}

function Write-Step([string]$Msg) {
    Write-Host "[deps] $Msg"
}

function Add-UserPath([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir)) { return }
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ([string]::IsNullOrEmpty($userPath)) {
        [Environment]::SetEnvironmentVariable("Path", $Dir, "User")
        $env:Path = ($env:Path.TrimEnd(';') + ";" + $Dir)
        Write-Step "PATH utilisateur : $Dir"
        return
    }
    $parts = $userPath.Split(';') | Where-Object { $_ -and $_.Trim() -ne "" }
    $norm = $Dir.TrimEnd('\').ToLowerInvariant()
    foreach ($p in $parts) {
        if ($p.TrimEnd('\').ToLowerInvariant() -eq $norm) { return }
    }
    $newPath = ($userPath.TrimEnd(';') + ";" + $Dir)
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = ($env:Path.TrimEnd(';') + ";" + $Dir)
    Write-Step "PATH utilisateur += $Dir"
}

function Get-ToolUrl([hashtable]$Meta) {
    return "https://github.com/$($Meta.Repo)/releases/latest/download/$($Meta.Asset).zip"
}

function Normalize-Version([string]$Raw) {
    if ([string]::IsNullOrWhiteSpace($Raw)) { return $null }
    $m = [regex]::Match($Raw, '(\d+\.\d+(?:\.\d+)?)')
    if (-not $m.Success) { return $null }
    return $m.Groups[1].Value.TrimStart('v', 'V')
}

function Compare-SemVer([string]$A, [string]$B) {
    # 0 égal, <0 A plus vieux, >0 A plus récent ; $null si incomparable
    $na = Normalize-Version $A
    $nb = Normalize-Version $B
    if (-not $na -or -not $nb) { return $null }
    try {
        $va = [version](($na.Split('.') + '0','0','0')[0..2] -join '.')
        $vb = [version](($nb.Split('.') + '0','0','0')[0..2] -join '.')
        return $va.CompareTo($vb)
    }
    catch {
        return $null
    }
}

function Get-LocalToolVersion([string]$ExeName, [string]$ToolsRoot, [string]$Name) {
    $candidates = @()
    $under = Join-Path (Join-Path $ToolsRoot $Name) $ExeName
    if (Test-Path -LiteralPath $under) { $candidates += $under }
    $cmd = Get-Command $ExeName -ErrorAction SilentlyContinue
    if ($cmd -and $cmd.Source) { $candidates += $cmd.Source }
    foreach ($path in $candidates) {
        try {
            $out = & $path --version 2>&1 | Out-String
            $v = Normalize-Version $out
            if ($v) { return @{ Version = $v; Path = $path } }
        }
        catch { }
    }
    return $null
}

function Get-LatestReleaseTag([string]$Repo) {
    try {
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
            -Headers @{ "User-Agent" = "HimaWeb-Installer"; "Accept" = "application/vnd.github+json" }
        return Normalize-Version $rel.tag_name
    }
    catch {
        Write-Warning "Impossible de lire la dernière release $Repo : $_"
        return $null
    }
}

function Test-NeedsInstallOrUpdate {
    param(
        [string]$Name,
        [hashtable]$Meta,
        [string]$ToolsRoot,
        [bool]$ForceInstall
    )
    if ($ForceInstall) {
        return @{ Need = $true; Reason = "force" }
    }
    $local = Get-LocalToolVersion -ExeName $Meta.Exe -ToolsRoot $ToolsRoot -Name $Name
    if (-not $local) {
        return @{ Need = $true; Reason = "absent" }
    }
    $latest = Get-LatestReleaseTag -Repo $Meta.Repo
    if (-not $latest) {
        # Pas de réseau / API : ne pas réinstaller si déjà présent
        Write-Step "$Name déjà installé ($($local.Version)) — skip (release distante inconnue)"
        return @{ Need = $false; Reason = "present-unknown-remote"; Local = $local.Version }
    }
    $cmp = Compare-SemVer $local.Version $latest
    if ($cmp -eq $null) {
        Write-Step "$Name local=$($local.Version) remote=$latest — versions non comparables, réinstall"
        return @{ Need = $true; Reason = "unparsed"; Local = $local.Version; Remote = $latest }
    }
    if ($cmp -ge 0) {
        Write-Step "$Name déjà à jour ($($local.Version))"
        return @{ Need = $false; Reason = "uptodate"; Local = $local.Version; Remote = $latest }
    }
    Write-Step "$Name à mettre à jour : $($local.Version) → $latest"
    return @{ Need = $true; Reason = "outdated"; Local = $local.Version; Remote = $latest }
}

# --- main ---

$toolsRoot = Join-Path $Prefix "tools"
New-Item -ItemType Directory -Force -Path $toolsRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Prefix "bin") | Out-Null

$toInstall = @()
$rawTools = @()
foreach ($t in $Tools) {
    if ([string]::IsNullOrWhiteSpace("$t")) { continue }
    $rawTools += ($t -split ',')
}
foreach ($t in $rawTools) {
    $key = "$t".Trim().ToLowerInvariant()
    if (-not $key) { continue }
    if (-not $Catalog.ContainsKey($key)) {
        Write-Warning "Outil inconnu ignoré : $t"
        continue
    }
    $check = Test-NeedsInstallOrUpdate -Name $key -Meta $Catalog[$key] -ToolsRoot $toolsRoot -ForceInstall:([bool]$Force)
    if (-not $check.Need) {
        # SkipIfPresent historique : même comportement (déjà couvert par uptodate/absent)
        Add-UserPath (Join-Path $toolsRoot $key)
        continue
    }
    if ($toInstall -notcontains $key) {
        $toInstall += $key
    }
}

if ($toInstall.Count -eq 0 -and -not $Ollama) {
    Write-Step "Rien à installer / mettre à jour."
    exit 0
}

$downloadRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("himaweb-deps-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $downloadRoot | Out-Null

try {
    $jobs = @()
    foreach ($name in $toInstall) {
        $meta = $Catalog[$name]
        $url = Get-ToolUrl $meta
        $zipPath = Join-Path $downloadRoot "$name.zip"
        Write-Step "Queue download $name"
        $jobs += Start-Job -Name "dl-$name" -ScriptBlock {
            param($Url, $OutFile)
            $ErrorActionPreference = "Stop"
            Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
        } -ArgumentList $url, $zipPath
    }

    if ($jobs.Count -gt 0) {
        Write-Step "Téléchargement parallèle ($($jobs.Count))…"
        Wait-Job -Job $jobs | Out-Null
        foreach ($j in $jobs) {
            if ($j.State -ne "Completed") {
                $err = Receive-Job $j 2>&1 | Out-String
                Write-Warning "Échec téléchargement $($j.Name) : $err"
            }
            else {
                Receive-Job $j | Out-Null
            }
            Remove-Job $j -Force -ErrorAction SilentlyContinue
        }
    }

    $anyFail = $false
    foreach ($name in $toInstall) {
        $meta = $Catalog[$name]
        $zipPath = Join-Path $downloadRoot "$name.zip"
        if (-not (Test-Path -LiteralPath $zipPath)) {
            Write-Warning "Pas d’archive pour $name — skip"
            $anyFail = $true
            continue
        }

        $extractDir = Join-Path $downloadRoot "extract-$name"
        New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
        try {
            Write-Step "Extraction $name …"
            Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

            $candidates = @($meta.Exe)
            if ($meta.ContainsKey("FindExe") -and $meta.FindExe) {
                $candidates = @($meta.FindExe) + @($meta.Exe)
            }
            $src = $null
            foreach ($cand in $candidates) {
                $src = Get-ChildItem -Path $extractDir -Filter $cand -Recurse -ErrorAction SilentlyContinue |
                    Select-Object -First 1
                if ($src) { break }
            }
            if (-not $src) {
                Write-Warning "Binaire introuvable pour $name"
                $anyFail = $true
                continue
            }

            $destDir = Join-Path $toolsRoot $name
            New-Item -ItemType Directory -Force -Path $destDir | Out-Null
            $destExe = Join-Path $destDir $meta.Exe
            Copy-Item -Path $src.FullName -Destination $destExe -Force
            Add-UserPath $destDir
            $ver = Get-LocalToolVersion -ExeName $meta.Exe -ToolsRoot $toolsRoot -Name $name
            if ($ver) {
                Write-Step "$name installé : $destExe ($($ver.Version))"
            }
            else {
                Write-Step "$name installé : $destExe"
            }
        }
        catch {
            Write-Warning "Échec $name : $_"
            $anyFail = $true
        }
    }

    if ($anyFail) {
        Write-Warning "Certaines dépendances ont échoué — HimaWeb peut quand même démarrer."
    }
}
finally {
    Remove-Item -Recurse -Force $downloadRoot -ErrorAction SilentlyContinue
}

if ($Ollama) {
    $ollamaCmd = Get-Command ollama.exe -ErrorAction SilentlyContinue
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($ollamaCmd -and -not $Force) {
        if ($winget) {
            Write-Step "Ollama déjà présent — tentative winget upgrade…"
            & winget upgrade -e --id Ollama.Ollama --accept-package-agreements --accept-source-agreements 2>$null
            if ($LASTEXITCODE -eq 0) {
                Write-Step "Ollama mis à jour (ou déjà à jour)."
            }
            else {
                Write-Step "Ollama déjà installé (pas de maj winget / déjà à jour)."
            }
        }
        else {
            Write-Step "Ollama déjà présent — skip"
        }
    }
    elseif ($winget) {
        Write-Step "Installation Ollama via winget…"
        & winget install -e --id Ollama.Ollama --accept-package-agreements --accept-source-agreements
    }
    else {
        Write-Step "winget introuvable — ouverture de https://ollama.com/download"
        Start-Process "https://ollama.com/download"
    }
}

Write-Step "Terminé. Configurez les comptes : himalaya configure / calendula configure / ortie configure"
