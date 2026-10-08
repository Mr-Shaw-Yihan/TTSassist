# Fish Audio TTS Plugin - Package Script
#
# Usage (in plugins/fish-audio-tts directory):
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1            # Package zip only
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1 -Install   # Package and install locally
#
# Close the app before using -Install (running dll cannot be overwritten).

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "fish-audio-tts"
$PluginName = "Fish Audio TTS"
$Version    = "0.1.0"
$MinAppVer  = "1.8.0"
$Desc       = "Fish Audio 云端语音合成（S2.1 系列），千款社区音色，需 API Key；默认免费模型 s2.1-pro-free，国内直连常超时、支持自动探测系统代理"

# 通用插件配置声明（宿主 ≥1.8.0 据此渲染设置面板并注入环境变量）
$ConfigDecl = @{
    help_url = "https://fish.audio"
    fields   = @(
        @{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "登录 fish.audio 后在 Settings → API Keys 创建（以 sk-fish- 开头）"
            placeholder = "sk-fish-..."
            env         = "FISH_AUDIO_API_KEY"
            required    = $true
        },
        @{
            key         = "model"
            type        = "select"
            label       = "合成模型"
            description = "免费档 s2.1-pro-free 无需账户余额；付费模型需账户有余额"
            env         = "FISH_AUDIO_MODEL"
            options     = @(
                @{ value = "s2.1-pro-free"; label = "S2.1 Pro Free（免费·默认）" },
                @{ value = "s2.1-pro";      label = "S2.1 Pro（付费）" },
                @{ value = "s2-pro";        label = "S2 Pro（付费）" },
                @{ value = "s1";            label = "S1（付费）" }
            )
        },
        @{
            key         = "latency"
            type        = "select"
            label       = "延迟档位"
            description = "Normal 质量优先；Low 首包更快"
            env         = "FISH_AUDIO_LATENCY"
            options     = @(
                @{ value = "normal";   label = "Normal（默认·质量优先）" },
                @{ value = "balanced"; label = "Balanced（均衡）" },
                @{ value = "low";      label = "Low（低延迟）" }
            )
        },
        @{
            key         = "voice_override"
            type        = "text"
            label       = "自定义音色 ID"
            description = "在 https://fish.audio/zh-CN/app/my-voices/ 查看你的音色模型 ID（32 位十六进制），填写后覆盖「当前音色」下拉；留空不覆盖"
            env         = "FISH_AUDIO_VOICE_OVERRIDE"
        },
        @{
            key         = "proxy"
            type        = "text"
            label       = "代理地址"
            description = "国内直连 api.fish.audio 常超时；如 http://127.0.0.1:7897，留空自动探测环境变量与系统代理"
            placeholder = "http://127.0.0.1:7897"
            env         = "FISH_AUDIO_PROXY"
        }
    )
}

$PluginDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WsTarget  = (Resolve-Path (Join-Path $PluginDir "..\..")).Path   # Cargo workspace 根（plugins/），共享构建产物 target 所在
$DistDir   = Join-Path $PluginDir "dist"
$StageDir  = Join-Path $DistDir "package"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# -- 1. Build --
Write-Host "[1/4] Building plugin (release)..." -ForegroundColor Cyan
Push-Location $PluginDir
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$DllSrc = Join-Path $WsTarget "target\release\fish_audio_tts.dll"
if (-not (Test-Path $DllSrc)) { throw "Build artifact not found: $DllSrc" }

# -- 2. Stage directory --
Write-Host "[2/4] Generating manifest.json (with SHA-256)..." -ForegroundColor Cyan
Remove-Item -Recurse -Force $DistDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item $DllSrc (Join-Path $StageDir "plugin.dll")

$Hash = (Get-FileHash (Join-Path $StageDir "plugin.dll") -Algorithm SHA256).Hash.ToLower()
$Manifest = [ordered]@{
    id              = $PluginId
    name            = $PluginName
    version         = $Version
    type            = "tts_engine"
    platform        = @("windows")
    entry           = "plugin.dll"
    min_app_version = $MinAppVer
    checksum        = $Hash
    description     = $Desc
    config          = $ConfigDecl
}
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 6),
    $Utf8NoBom
)

# -- 3. Compress zip --
Write-Host "[3/4] Packaging zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -Force
Write-Host "Package complete: $ZipPath" -ForegroundColor Green
Write-Host "  plugin.dll SHA-256: $Hash"

# Sync to app resource directory
$ResDir = Join-Path $PluginDir "..\..\..\src-tauri\resources\plugins"
New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
Remove-Item (Join-Path $ResDir "$PluginId-*.zip") -ErrorAction SilentlyContinue
Copy-Item $ZipPath $ResDir -Force
Write-Host "Synced to resources: $ResDir" -ForegroundColor Green

# -- 4. Optional: Install locally --
if ($Install) {
    Write-Host "[4/4] Installing locally..." -ForegroundColor Cyan

    if (Get-Process -Name "TTSassist", "voiceassist", "TTKook-genie" -ErrorAction SilentlyContinue) {
        throw "App is running. Close it first, then re-run with -Install"
    }

    $RepoRoot = (Resolve-Path (Join-Path $PluginDir "..\..\..")).Path
    $ExePaths = @(
        (Join-Path $RepoRoot "src-tauri\target\release\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\release\TTKook-genie.exe"),
        (Join-Path $RepoRoot "src-tauri\target\release\voiceassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\TTKook-genie.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\voiceassist.exe")
    )
    $ExePath = $ExePaths | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $ExePath) {
        throw "No exe found (build first). Or set VA_PLUGINS_DIR env var."
    }
    if ($env:VA_PLUGINS_DIR) {
        $PluginsDir = $env:VA_PLUGINS_DIR
    } else {
        $PluginsDir = Join-Path (Split-Path -Parent $ExePath) "plugins"
    }
    $TargetDir = Join-Path $PluginsDir $PluginId
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

    Copy-Item (Join-Path $StageDir "plugin.dll")    $TargetDir -Force
    Copy-Item (Join-Path $StageDir "manifest.json") $TargetDir -Force

    # Update registry.json
    $RegPath = Join-Path $PluginsDir "registry.json"
    if (Test-Path $RegPath) {
        $Reg = Get-Content $RegPath -Raw | ConvertFrom-Json
    } else {
        $Reg = [PSCustomObject]@{ plugins = @() }
    }
    $Entries = @()
    if ($Reg.plugins) {
        $Entries += @($Reg.plugins | Where-Object { $_.id -ne $PluginId })
    }
    $Entries += [PSCustomObject]@{
        id           = $PluginId
        version      = $Version
        installed_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:sszzz")
    }
    $Reg | Add-Member -NotePropertyName "plugins" -NotePropertyValue @($Entries) -Force
    [System.IO.File]::WriteAllText($RegPath, ($Reg | ConvertTo-Json -Depth 5), $Utf8NoBom)

    Write-Host "Installed to: $TargetDir" -ForegroundColor Green
    Write-Host "Launch the app and select '$PluginName' in engine settings."
}
