# verify-index.ps1 - Consistency guard for the plugin online index (plugins-index.json).
#
# Two independent parts, each contributing to the failure count:
#   Part A  index <-> src-tauri/resources/plugins/*.zip   (CI-safe: only committed files)
#           Every index entry's <id>-<version>.zip must exist under resources and its
#           sha256 must equal index.checksum; every resources zip must be referenced by
#           the index (else it is a stale leftover). Catches "installer bundles a zip the
#           index does not point at" (v1.8.0 incident class).
#   Part B  index <-> built plugins/<id>/dist (manifest.json + zip)   (dev / publish time)
#           Requires the plugin to have been packaged (dist/package/manifest.json present).
#           On a fresh CI checkout there is no dist, so Part B is skipped automatically.
#           Catches "index frozen behind a new build" (v1.8.2 incident class).
#
# Exit code 0 = all consistent; 1 = drift detected.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-index.ps1                 # full (dev box with dist built)
#   powershell ... -File .\verify-index.ps1 -SkipDist                                       # CI / fresh clone
#   powershell ... -File .\verify-index.ps1 -SkipResources                                  # plugins-only publish (resources not synced yet)
#   powershell ... -File .\verify-index.ps1 -IndexPath <path>                               # point at another index (CI/tests)

param(
    [switch]$SkipResources,
    [switch]$SkipDist,
    [string]$IndexPath
)

$ErrorActionPreference = 'Stop'
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $PluginsDir
if (-not $IndexPath) { $IndexPath = Join-Path $PluginsDir 'plugins-index.json' }
$ResDir     = Join-Path $RepoRoot 'src-tauri\resources\plugins'

function Get-ZipSha([string]$p) { (Get-FileHash $p -Algorithm SHA256).Hash.ToLower() }

if (-not (Test-Path $IndexPath)) { Write-Host "INDEX FILE MISSING: $IndexPath" -ForegroundColor Red; exit 1 }
$indexPlugins = @((Get-Content $IndexPath -Raw -Encoding UTF8 | ConvertFrom-Json).plugins)
$byId = @{}
foreach ($e in $indexPlugins) { $byId[$e.id] = $e }

$rows = @(); $fail = 0

# ---------- Part A: index <-> resources/plugins ----------
if (-not $SkipResources) {
    $resFiles = @{}
    if (Test-Path $ResDir) {
        Get-ChildItem $ResDir -Filter '*.zip' | ForEach-Object { $resFiles[$_.Name] = $_ }
    }
    $wanted = @{}
    foreach ($e in $indexPlugins) {
        $zipName = "$($e.id)-$($e.version).zip"
        $wanted[$zipName] = $true
        $st = 'OK'; $d = ''
        if (-not $resFiles.ContainsKey($zipName)) { $st = 'RES-MISSING'; $d = "resources/plugins/$zipName absent"; $fail++ }
        elseif ((Get-ZipSha $resFiles[$zipName].FullName) -ne $e.checksum.ToLower()) { $st = 'RES-CK'; $d = 'resources zip sha != index.checksum'; $fail++ }
        $rows += [pscustomobject]@{ Scope='A index->res'; Id=$e.id; Ver=$e.version; Status=$st; Detail=$d }
    }
    foreach ($name in $resFiles.Keys) {
        if (-not $wanted.ContainsKey($name)) {
            $idGuess = ($name -replace '-\d.*$', '')
            $rows += [pscustomobject]@{ Scope='A res->index'; Id=$idGuess; Ver=''; Status='RES-STALE'; Detail="$name not referenced by index (stale leftover)"; }
            $fail++
        }
    }
}

# ---------- Part B: index <-> built dist ----------
if (-not $SkipDist) {
    $dirs = @(Get-ChildItem $PluginsDir -Directory | Where-Object {
        (Test-Path (Join-Path $_.FullName 'package.ps1')) -and
        (Test-Path (Join-Path $_.FullName 'dist\package\manifest.json'))
    } | Sort-Object Name)
    if ($dirs.Count -eq 0) {
        Write-Host '(no built dist/package/manifest.json found - Part B skipped; run each package.ps1 first. Expected on a fresh CI checkout.)' -ForegroundColor DarkYellow
    }
    $seen = @{}
    foreach ($dir in $dirs) {
        $man = Get-Content (Join-Path $dir.FullName 'dist\package\manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $id = $dir.Name; $ver = $man.version; $seen[$id] = $true
        $st = 'OK'; $d = ''
        $e = $byId[$id]
        if (-not $e) { $st = 'DRIFT-MISSING'; $d = 'built plugin not in plugins-index.json'; $fail++ }
        elseif ($e.version -ne $ver) { $st = 'VER-MISMATCH'; $d = "index=$($e.version) manifest=$ver"; $fail++ }
        else {
            $z = Get-ChildItem (Join-Path $dir.FullName 'dist') -Filter "$id-$ver.zip" -EA SilentlyContinue | Select-Object -First 1
            if (-not $z) { $st = 'ZIP-MISSING'; $d = "dist/$id-$ver.zip absent"; $fail++ }
            elseif ((Get-ZipSha $z.FullName) -ne $e.checksum.ToLower()) { $st = 'CK-DIST'; $d = 'dist zip sha != index.checksum'; $fail++ }
        }
        $rows += [pscustomobject]@{ Scope='B index<->dist'; Id=$id; Ver=$ver; Status=$st; Detail=$d }
    }
    # reverse: index entries with no built plugin dir (only meaningful when some dist present)
    if ($dirs.Count -gt 0) {
        foreach ($e in $indexPlugins) {
            if (-not $seen[$e.id]) { $rows += [pscustomobject]@{ Scope='B orphan'; Id=$e.id; Ver=$e.version; Status='ORPHAN-IN-INDEX'; Detail='index references a plugin with no built manifest' }; $fail++ }
        }
    }
}

$rows | Format-Table -AutoSize | Out-String | Write-Host
Write-Host ("index entries = {0}   failures = {1}   (Part A SkipResources={2}, Part B SkipDist={3})" -f $indexPlugins.Count, $fail, [bool]$SkipResources, [bool]$SkipDist)
if ($fail -eq 0) { Write-Host 'RESULT: ALL CONSISTENT' -ForegroundColor Green; exit 0 }
else { Write-Host 'RESULT: DRIFT DETECTED' -ForegroundColor Red; exit 1 }
