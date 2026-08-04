# Edge TTS 插件打包脚本（PowerShell）
#
# 用法（在 plugins/edge-tts 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 只打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本机 VoiceAssist
#
# -Install 会装进 %APPDATA%/com.voiceassist.app/plugins/edge-tts/ 并更新 registry.json。
# 安装前请先关闭 VoiceAssist（运行中的 dll 被占用无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "edge-tts"
$PluginName = "Edge TTS（免费·微软）"
$Version    = "1.0.0"
$MinAppVer  = "1.3.1"
$Desc       = "免费、无需 Key 的微软 Edge 语音，非官方接口可能不稳定"

$PluginDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$DistDir   = Join-Path $PluginDir "dist"
$StageDir  = Join-Path $DistDir "package"
# 无 BOM 的 UTF-8（BOM 会让宿主的 JSON 解析失败）
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# ── 1. 构建 ──────────────────────────────────────────
Write-Host "[1/4] 构建插件（release）..." -ForegroundColor Cyan
Push-Location $PluginDir
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败" }
} finally {
    Pop-Location
}

$DllSrc = Join-Path $PluginDir "target\release\edge_tts.dll"
if (-not (Test-Path $DllSrc)) { throw "找不到构建产物: $DllSrc" }

# ── 2. 暂存目录（zip 内容：平铺 manifest.json + plugin.dll）──
Write-Host "[2/4] 生成 manifest.json（含 SHA-256）..." -ForegroundColor Cyan
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
}
# 注意：必须用无 BOM 写入（Set-Content -Encoding UTF8 在 PS 5.1 会带 BOM，宿主的 JSON 解析器不认）
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json),
    $Utf8NoBom
)

# ── 3. 压缩 zip ──────────────────────────────────────
Write-Host "[3/4] 打包 zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -Force
Write-Host "打包完成: $ZipPath" -ForegroundColor Green
Write-Host "  plugin.dll SHA-256: $Hash"

# ── 4. 可选：安装到本机 VoiceAssist ──────────────────
if ($Install) {
    Write-Host "[4/4] 安装到本机 VoiceAssist..." -ForegroundColor Cyan

    # 检查应用是否在运行（dll 占用会导致覆盖失败）
    if (Get-Process -Name "TTSassist", "voiceassist" -ErrorAction SilentlyContinue) {
        throw "VoiceAssist 正在运行，请先关闭再执行 -Install"
    }

    $DataDir    = Join-Path $env:APPDATA "com.voiceassist.app"
    $TargetDir  = Join-Path $DataDir "plugins\$PluginId"
    $PluginsDir = Join-Path $DataDir "plugins"
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

    Copy-Item (Join-Path $StageDir "plugin.dll")    $TargetDir -Force
    Copy-Item (Join-Path $StageDir "manifest.json") $TargetDir -Force

    # 更新 registry.json（已有同 id 记录则替换）
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

    Write-Host "已安装到: $TargetDir" -ForegroundColor Green
    Write-Host "启动 VoiceAssist 后，在设置引擎处选择「$PluginName」即可使用。"
}
