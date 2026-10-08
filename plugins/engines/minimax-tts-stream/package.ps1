# MiniMax 流式 TTS 插件 - 打包脚本（模板同 minimax-tts/package.ps1）
#
# 用法（在 plugins/minimax-tts-stream 目录）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 仅打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本地 exe 同级 plugins/
#
# -Install 前先关闭主程序（运行中的 dll 无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "minimax-tts-stream"
$PluginName = "MiniMax 流式 TTS（边合边播）"
$Version    = "0.2.0"
$MinAppVer  = "1.8.0"
$Desc       = "MiniMax 云端语音合成流式版（国内版）：WebSocket 边合成边出块，首响更快。需 API Key，50+ 系统音色 + 账号克隆音色。配置可切回非流式对比"

# 通用插件配置声明（宿主 ≥1.8.0 据此渲染设置面板并注入环境变量）。
# 注意 env 名独立于 minimax-tts / minimax-tts-global，避免宿主必填 env 冲突检测拒载。
# stream 字段由宿主读取 plugin_config[engine].stream 决定流式/阻塞管线（env 声明为占位，插件不消费）。
$ConfigDecl = @{
    help_url = "https://platform.minimaxi.com/user-center/basic-information/interface-key"
    fields   = @(
        @{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "从 MiniMax 开放平台（国内版）获取"
            env         = "MINIMAX_STREAM_API_KEY"
            required    = $true
        },
        @{
            key         = "stream"
            type        = "select"
            label       = "合成模式"
            description = "流式：边合成边播放，首响更快；非流式：整段合成后播放。用于同引擎前后对比（默认流式）"
            env         = "MINIMAX_STREAM_MODE"
            required    = $false
            options     = @(
                @{ value = "on";  label = "流式（边合边播·默认）" },
                @{ value = "off"; label = "非流式（整段对比）" }
            )
        },
        @{
            key         = "model"
            type        = "select"
            label       = "模型"
            description = "合成模型（默认 speech-2.8-hd）"
            env         = "MINIMAX_STREAM_MODEL"
            required    = $false
            options     = @(
                @{ value = "speech-2.8-hd";     label = "speech-2.8-hd（高品质）" },
                @{ value = "speech-2.8-turbo";  label = "speech-2.8-turbo（低延迟）" },
                @{ value = "speech-02-hd";      label = "speech-02-hd" },
                @{ value = "speech-02-turbo";   label = "speech-02-turbo" }
            )
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

$DllSrc = Join-Path $WsTarget "target\release\minimax_tts_stream.dll"
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
