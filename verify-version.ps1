# verify-version.ps1 - Assert the desktop app version is identical across the FOUR
# required files (see doc/发布流程.md §一):
#   package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml ([package]),
#   src-tauri/Cargo.lock (voiceassist block).
# Read-only; exit 0 = all equal, exit 1 = desync. CI-safe and locally runnable.
# Bump with: .\bump-version.ps1 X.Y.Z
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-version.ps1

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path

function Get-Ver([string]$path, [string]$pattern) {
    if (-not (Test-Path $path)) { return '(file missing)' }
    $text = Get-Content $path -Raw -Encoding UTF8
    $m = [regex]::Match($text, $pattern)
    if ($m.Success) { return $m.Groups[1].Value } else { return '(NOT FOUND)' }
}

$checks = @(
    @{ Name='package.json';           Path=(Join-Path $Root 'package.json');            Pat='"version"\s*:\s*"([^"]+)"' },
    @{ Name='tauri.conf.json';        Path=(Join-Path $Root 'src-tauri\tauri.conf.json'); Pat='"version"\s*:\s*"([^"]+)"' },
    @{ Name='Cargo.toml [package]';   Path=(Join-Path $Root 'src-tauri\Cargo.toml');    Pat='(?m)^version\s*=\s*"([^"]+)"' },
    @{ Name='Cargo.lock voiceassist'; Path=(Join-Path $Root 'src-tauri\Cargo.lock');    Pat='(?m)name\s*=\s*"voiceassist"\r?\nversion\s*=\s*"([^"]+)"' }
)

$vals = @()
foreach ($c in $checks) {
    $v = Get-Ver $c.Path $c.Pat
    $vals += [pscustomobject]@{ File=$c.Name; Version=$v }
}
$vals | Format-Table -AutoSize | Out-String | Write-Host

$distinct = @($vals.Version | Sort-Object -Unique)
$bad = @($vals | Where-Object { $_.Version -notmatch '^\d+\.\d+\.\d+$' })
if ($distinct.Count -eq 1 -and $bad.Count -eq 0) {
    Write-Host ("RESULT: VERSION CONSISTENT = {0}" -f $distinct[0]) -ForegroundColor Green
    exit 0
}
if ($bad.Count -gt 0) { Write-Host 'RESULT: one or more versions unreadable/invalid' -ForegroundColor Red }
else { Write-Host ("RESULT: VERSION DESYNC -> {0}" -f ($distinct -join ' , ')) -ForegroundColor Red }
Write-Host 'Fix with: powershell -File .\bump-version.ps1 <x.y.z>'
exit 1
