# Plugin Publish Script - Upload minimax plugins to GitHub Release + Gitee mirror
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1                # Publish all
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -DryRun        # Only generate index, skip upload
#
# Prerequisites: gh CLI installed and authenticated.

param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$RepoBase  = "https://github.com/Mr-Shaw-Yihan/TTSassist"
$GiteeBase = "https://gitee.com/yihwan/TTSassist"
$GiteeRemote = "gitee"
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $PluginsDir
$Utf8NoBom  = New-Object System.Text.UTF8Encoding($false)

# All publishable plugins
$Plugins = @(
    @{ Id = "edge-tts";         Name = "Edge TTS (Free/Microsoft)";      Type = "tts_engine"; Desc = "Free Microsoft Edge TTS, no API key required (unofficial API)" },
    @{ Id = "genie-tts";        Name = "Genie TTS (Local/Offline)";      Type = "tts_engine"; Desc = "GPT-SoVITS ONNX local inference, CPU offline, expandable voice packs" },
    @{ Id = "minimax-tts";      Name = "MiniMax TTS（国内版）";         Type = "tts_engine"; Desc = "MiniMax 云端语音合成（国内版·阉割版），需 API Key，50+ 音色、40 种语言，不支持音色克隆" },
    @{ Id = "minimax-tts-global"; Name = "MiniMax TTS（国际版）";         Type = "tts_engine"; Desc = "MiniMax 云端语音合成（国际版），需 API Key，50+ 音色、40 种语言，支持音色克隆与音色管理" }
)

# -- 1. Package all plugins --
$Entries = @()
$Assets  = @()
foreach ($p in $Plugins) {
    Write-Host ""
    Write-Host "== Packaging $($p.Id) ==" -ForegroundColor Cyan

    $PkgScript = Join-Path $PluginsDir "$($p.Id)\package.ps1"
    if (Test-Path $PkgScript) {
        & $PkgScript
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { throw "$($p.Id) packaging failed" }
    }

    $Zip = Get-ChildItem (Join-Path $PluginsDir "$($p.Id)\dist") -Filter "$($p.Id)-*.zip" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $Zip) { throw "No zip found for $($p.Id)" }

    $Version = $Zip.BaseName.Substring($p.Id.Length + 1)
    $Sha = (Get-FileHash $Zip.FullName -Algorithm SHA256).Hash.ToLower()

    $Entries += [ordered]@{
        id           = $p.Id
        name         = $p.Name
        version      = $Version
        download_url = "$RepoBase/releases/latest/download/$($Zip.Name)"
        mirror_url   = "$GiteeBase/raw/dist/$($Zip.Name)"
        checksum     = $Sha
        description  = $p.Desc
        plugin_type  = $p.Type
    }
    $Assets += $Zip.FullName
    Write-Host "  $($p.Id) v$Version  SHA-256: $Sha" -ForegroundColor Green
}

# -- 2. Generate plugins-index.json --
$Index = [ordered]@{ plugins = @($Entries) }
$IndexPath = Join-Path $PluginsDir "plugins-index.json"
[System.IO.File]::WriteAllText($IndexPath, ($Index | ConvertTo-Json -Depth 5), $Utf8NoBom)
Write-Host ""
Write-Host "Index generated: $IndexPath" -ForegroundColor Green
$Assets += $IndexPath

if ($DryRun) {
    Write-Host "DryRun: skipping upload." -ForegroundColor Yellow
    return
}

# -- 3. Upload to latest GitHub Release --
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "gh CLI not found. Install: winget install GitHub.cli; gh auth login"
}

# Find latest release tag
$LatestTag = (gh release list --limit 1 --json tagName --jq ".[0].tagName" 2>&1 | Out-String).Trim()
if (-not $LatestTag) { throw "No release found on GitHub" }
Write-Host ""
Write-Host "Uploading to latest release: $LatestTag" -ForegroundColor Cyan

$ErrorActionPreference = "Continue"
gh release upload $LatestTag @Assets --clobber 2>&1 | ForEach-Object { "$_" }
$UploadOk = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = "Stop"
if (-not $UploadOk) { throw "gh release upload failed" }

Write-Host "Upload complete!" -ForegroundColor Green
Write-Host "Index URL: $RepoBase/releases/latest/download/plugins-index.json"

# -- 4. Sync Gitee dist branch --
# 用临时仓库推送，不动主仓库工作区（避免 orphan + git rm 清掉未提交改动）
Write-Host ""
Write-Host "== Syncing Gitee dist branch ==" -ForegroundColor Cyan
$DistWork = Join-Path $env:TEMP "va-dist-push"
if (Test-Path $DistWork) { Remove-Item $DistWork -Recurse -Force }
New-Item -ItemType Directory -Path $DistWork | Out-Null
foreach ($a in $Assets) { Copy-Item $a -Destination $DistWork -Force }

Push-Location $RepoRoot
$GiteeUrl = (git remote get-url $GiteeRemote 2>&1 | Out-String).Trim()
Pop-Location
if (-not $GiteeUrl) { throw "Remote '$GiteeRemote' not found in repo" }

Push-Location $DistWork
try {
    git init --quiet
    git add . 2>&1 | Out-Null
    git commit -m "dist: plugin mirror (index + zips)" --quiet
    $ErrorActionPreference = "Continue"
    git push $GiteeUrl HEAD:dist --force 2>&1 | ForEach-Object { "$_" }
    $PushOk = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = "Stop"
    if (-not $PushOk) { throw "Failed to push dist branch to Gitee" }
} finally {
    Pop-Location
    Remove-Item $DistWork -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "Gitee dist synced! Mirror: $GiteeBase/raw/dist/plugins-index.json" -ForegroundColor Green
