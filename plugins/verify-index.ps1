# verify-index.ps1 - Read-only consistency guard for the plugin online index.
#
# Catches the exact class of bug that caused the v1.8.2 incident: the built
# plugin (dist/package/manifest.json + dist zip) drifting out of sync with
# plugins-index.json and src-tauri/resources/plugins.
#
# Asserts, for every publishable plugin (a dir containing BOTH package.ps1 and
# dist/package/manifest.json):
#   * it has an entry in plugins-index.json           (else DRIFT-MISSING)
#   * index.version  == manifest.version              (else VER-MISMATCH)
#   * dist zip <id>-<ver>.zip exists and sha256==index.checksum  (else CK-DIST)
#   * resources zip <id>-<ver>.zip exists and sha256==index.checksum (else RES-*)
# And in reverse: no index entry lacks a built plugin (else ORPHAN-IN-INDEX).
#
# Exit code 0 = all consistent; 1 = drift detected (usable in CI / pre-commit).
#
# Usage (from anywhere):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-index.ps1
#   powershell ... -File .\verify-index.ps1 -SkipResources   # ignore resources/ (plugins-only release)

param(
    [switch]$SkipResources,
    [string]$IndexPath
)

$ErrorActionPreference = 'Stop'
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $PluginsDir
if (-not $IndexPath) { $IndexPath = Join-Path $PluginsDir 'plugins-index.json' }
$ResDir     = Join-Path $RepoRoot 'src-tauri\resources\plugins'

function Get-ZipSha([string]$path) { (Get-FileHash $path -Algorithm SHA256).Hash.ToLower() }

if (-not (Test-Path $IndexPath)) { Write-Host "INDEX FILE MISSING: $IndexPath" -ForegroundColor Red; exit 1 }
$indexPlugins = (Get-Content $IndexPath -Raw -Encoding UTF8 | ConvertFrom-Json).plugins
$byId = @{}
foreach ($e in $indexPlugins) { $byId[$e.id] = $e }

# Discover publishable plugins by manifest presence (single source of truth).
$dirs = Get-ChildItem $PluginsDir -Directory | Where-Object {
    (Test-Path (Join-Path $_.FullName 'package.ps1')) -and
    (Test-Path (Join-Path $_.FullName 'dist\package\manifest.json'))
} | Sort-Object Name

$rows = @(); $fail = 0
$seenIds = @{}
foreach ($d in $dirs) {
    $man = Get-Content (Join-Path $d.FullName 'dist\package\manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $id = $d.Name; $seenIds[$id] = $true
    $ver = $man.version
    $status = 'OK'; $detail = ''
    $entry = $byId[$id]
    if (-not $entry) {
        $status = 'DRIFT-MISSING'; $detail = "not in plugins-index.json"; $fail++
    } elseif ($entry.version -ne $ver) {
        $status = 'VER-MISMATCH'; $detail = "index=$($entry.version) manifest=$ver"; $fail++
    } else {
        # dist zip
        $distZip = Get-ChildItem (Join-Path $d.FullName 'dist') -Filter "$id-$ver.zip" -EA SilentlyContinue | Select-Object -First 1
        if (-not $distZip) { $status = 'ZIP-MISSING'; $detail = "dist/$id-$ver.zip absent"; $fail++ }
        else {
            $sha = Get-ZipSha $distZip.FullName
            if ($sha -ne $entry.checksum.ToLower()) { $status = 'CK-DIST'; $detail = "zip sha != index.checksum"; $fail++ }
        }
        # resources zip
        if ($status -eq 'OK' -and -not $SkipResources) {
            $resZip = Join-Path $ResDir "$id-$ver.zip"
            if (-not (Test-Path $resZip)) { $status = 'RES-MISSING'; $detail = "resources/plugins/$id-$ver.zip absent"; $fail++ }
            elseif ((Get-ZipSha $resZip) -ne $entry.checksum.ToLower()) { $status = 'RES-CK'; $detail = "resources zip sha != index.checksum"; $fail++ }
        }
    }
    $rows += [pscustomobject]@{ Id=$id; Ver=$ver; Status=$status; Detail=$detail }
}

# Reverse: index entries with no built plugin dir
foreach ($e in $indexPlugins) {
    if (-not $seenIds[$e.id]) {
        $rows += [pscustomobject]@{ Id=$e.id; Ver=$e.version; Status='ORPHAN-IN-INDEX'; Detail='index references a plugin with no built manifest' }
        $fail++
    }
}

$rows | Format-Table -AutoSize | Out-String | Write-Host
Write-Host ("plugins checked = {0}   index entries = {1}   failures = {2}" -f $dirs.Count, $indexPlugins.Count, $fail)
if ($fail -eq 0) { Write-Host 'RESULT: ALL CONSISTENT' -ForegroundColor Green; exit 0 }
else { Write-Host 'RESULT: DRIFT DETECTED' -ForegroundColor Red; exit 1 }
