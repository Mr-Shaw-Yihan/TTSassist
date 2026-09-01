# bump-version.ps1 - Sync the desktop app version across all required files.
#
# The project keeps the version in FOUR places that must stay equal
# (see doc/发布流程.md §一): package.json, src-tauri/tauri.conf.json,
# src-tauri/Cargo.toml ([package]), src-tauri/Cargo.lock (voiceassist block).
# Manual editing of four files is error-prone; this tool does it in one shot
# and verifies the result. It touches ONLY these four; the Flutter remote-app
# (remote-app/pubspec.yaml) has its own version cadence managed by the App
# release flow and is intentionally NOT modified here.
#
# Usage (from VoiceAssist repo root or anywhere):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\bump-version.ps1 1.8.4 -DryRun
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\bump-version.ps1 1.8.4
#   # after bumping, Cargo.lock is already synced; no `cargo` run needed.

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "版本号格式应为 x.y.z（数字三段），收到：$Version"
}

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Files = [ordered]@{
    'package.json'            = Join-Path $Root 'package.json'
    'tauri.conf.json'         = Join-Path $Root 'src-tauri\tauri.conf.json'
    'Cargo.toml [package]'    = Join-Path $Root 'src-tauri\Cargo.toml'
    'Cargo.lock voiceassist'  = Join-Path $Root 'src-tauri\Cargo.lock'
}
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# Each entry: path key -> regex with two capture groups ($1 prefix, $2 suffix).
# Replace applies to the FIRST match only.
function Get-Current([string]$key, [string]$path) {
    $text = Get-Content $path -Raw -Encoding UTF8
    switch ($key) {
        'package.json'           { $m = [regex]::Match($text, '"version"\s*:\s*"([^"]+)"') }
        'tauri.conf.json'        { $m = [regex]::Match($text, '"version"\s*:\s*"([^"]+)"') }
        'Cargo.toml [package]'   { $m = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"') }
        'Cargo.lock voiceassist' { $m = [regex]::Match($text, '(?m)name\s*=\s*"voiceassist"\r?\nversion\s*=\s*"([^"]+)"') }
    }
    if ($m.Success) { return $m.Groups[1].Value } else { return '(NOT FOUND)' }
}

function Set-Version([string]$key, [string]$path, [string]$ver) {
    $text = Get-Content $path -Raw -Encoding UTF8
    if ($key -eq 'Cargo.lock voiceassist') {
        $rx = [regex]'(?m)(name\s*=\s*"voiceassist"\r?\nversion\s*=\s*)"[^"]+"'
        $new = $rx.Replace($text, ('${1}"' + $ver + '"'), 1)
    } else {
        $pat = switch ($key) {
            'Cargo.toml [package]' { '(?m)^version\s*=\s*"[^"]+"' }
            default                { '"version"\s*:\s*"[^"]+"' }
        }
        $rx = [regex]$pat
        # keep the matched assignment intact; swap only the trailing quoted value
        $new = $rx.Replace($text, { param($mm) $mm.Value -replace '"[^"]*"$', ('"' + $ver + '"') }, 1)
    }
    if ($new -eq $text) { throw "未能在 $path 定位版本行（正则未命中），已中止" }
    [System.IO.File]::WriteAllText($path, $new, $Utf8NoBom)
}

Write-Host ''
Write-Host ("=== 版本同步目标: {0} {1} ===" -f $Version, $(if ($DryRun) { '(DryRun 预演，不写文件)' } else { '' }))
$before = @{}
foreach ($k in $Files.Keys) { $before[$k] = Get-Current $k $Files[$k] }
foreach ($k in $Files.Keys) {
    Write-Host ("  {0,-26} {1} -> {2}" -f $k, $before[$k], $Version)
}

if ($DryRun) { Write-Host "`nDryRun：未改动任何文件。去掉 -DryRun 以实际写入。" -ForegroundColor Yellow; return }

foreach ($k in $Files.Keys) { Set-Version $k $Files[$k] $Version }

# verify
Write-Host ''
$ok = $true
foreach ($k in $Files.Keys) {
    $now = Get-Current $k $Files[$k]
    if ($now -ne $Version) { $ok = $false; Write-Host ("  MISMATCH {0}: {1}" -f $k, $now) -ForegroundColor Red }
    else { Write-Host ("  OK  {0,-26} = {1}" -f $k, $now) -ForegroundColor Green }
}
if (-not $ok) { throw "四处版本未全部同步成功，请检查上面 MISMATCH 的文件" }
Write-Host ''
Write-Host "四处版本已同步为 $Version ✅" -ForegroundColor Green
Write-Host "提示：本次仅改桌面端 4 处；Flutter 遥控 App 版本（remote-app/pubspec.yaml）走 App 发布流程单独管理。"
Write-Host "提示：若插件有改动，发版前记得跑 plugins/verify-index.ps1，并按发布流程走 -SyncResources。"
