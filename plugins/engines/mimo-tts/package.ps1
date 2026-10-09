# MiMo TTS（小米·流式）插件 - 打包脚本
#
# 用法（在 plugins/engines/mimo-tts 目录）：
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1            # 仅打包 zip
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本地 exe 同级 plugins/
#
# -Install 前先关闭主程序（运行中的 dll 无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "mimo-tts"
$PluginName = "MiMo TTS（小米·流式）"
$Version    = "0.1.0"
$MinAppVer  = "1.8.0"
$Desc       = "小米 MiMo 云端语音合成（流式版）：SSE 边合成边播，首响更快。需 API Key，预置音色 + 本地样本音色克隆（克隆由服务端退化为兼容模式，首响较慢）"

# 通用插件配置声明（宿主 ≥1.8.0 据此渲染设置面板并注入环境变量）。
# env 名必须独立于 mimo-asr 的 MIMO_API_KEY：两个必填 env 同名会触发宿主
# find_required_env_conflict 直接拒载（见 plugins/config.rs）。
# model 用 text 而非 select：manifest 校验强制 select 必须给出非空 options，
# 而官方档位会变；text 才能做到"上新档位改一个字符串、不发版"。
# help_url 是静态兜底，运行期会被远程配置 plugin_help_url 覆盖（换邀请码免发版）。
$ConfigDecl = @{
    help_url = "https://platform.xiaomimimo.com?ref=5P9J2B"
    fields   = @(
        @{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "从小米 MiMo 开放平台获取（与 MiMo ASR 用的是同一个 Key）"
            env         = "MIMO_TTS_API_KEY"
            required    = $true
        },
        @{
            key         = "model"
            type        = "text"
            label       = "合成模型"
            description = "留空即 mimo-v2.5-tts；官方上新档位可直接在此改名，无需更新软件"
            env         = "MIMO_TTS_MODEL"
            placeholder = "mimo-v2.5-tts"
            required    = $false
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

$DllSrc = Join-Path $WsTarget "target\release\mimo_tts.dll"
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
    category        = "remote"
    config          = $ConfigDecl
}
# 必须 -Depth 6：默认 Depth=2 会把 config.fields 的嵌套结构截成字符串
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

# Sync to app resource directory（内置分发：随安装包携带，可卸载）
$ResDir = Join-Path $PluginDir "..\..\..\src-tauri\resources\plugins"
New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
Remove-Item (Join-Path $ResDir "$PluginId-*.zip") -ErrorAction SilentlyContinue
Copy-Item $ZipPath $ResDir -Force
Write-Host "Synced to resources: $ResDir" -ForegroundColor Green
Write-Host ("  index.checksum must be zip SHA-256: {0}" -f (Get-FileHash $ZipPath -Algorithm SHA256).Hash.ToLower()) -ForegroundColor Yellow

# -- 4. Optional: Install locally --
if ($Install) {
    Write-Host "[4/4] Installing locally..." -ForegroundColor Cyan

    if (Get-Process -Name "TTSassist", "voiceassist" -ErrorAction SilentlyContinue) {
        throw "App is running. Close it first, then re-run with -Install"
    }

    $RepoRoot = (Resolve-Path (Join-Path $PluginDir "..\..\..")).Path
    $ExePaths = @(
        (Join-Path $RepoRoot "src-tauri\target\release\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\release\voiceassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\TTSassist.exe"),
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
