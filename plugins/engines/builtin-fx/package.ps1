# builtin-fx 内置效果器包打包脚本（PowerShell）
#
# 用法（在 plugins/builtin-fx 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 只打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本机 VoiceAssist
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -ToResources  # 打包并复制进 src-tauri/resources/plugins（随安装包内置分发）
#
# zip 内容平铺：manifest.json + plugin.dll。manifest 必须 UTF-8 无 BOM；
# 本脚本自身必须存为 UTF-8 带 BOM（含中文注释，PS 5.1 兼容）。

param(
    [switch]$Install,
    [switch]$ToResources
)

$ErrorActionPreference = "Stop"

$PluginId   = "builtin-fx"
$PluginName = "内置效果器包"
$Version    = "1.0.0"
$MinAppVer  = "1.9.0"
$Desc       = "随宿主内置的人声效果预设：电音、花栗鼠、低音炮、失真电子声、电话音"

$PluginDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WsTarget  = (Resolve-Path (Join-Path $PluginDir "..\..")).Path   # Cargo workspace 根（plugins/），共享构建产物 target 所在
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

$DllSrc = Join-Path $WsTarget "target\release\builtin_fx.dll"
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
    type            = "audio_effect"
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

# ── 3. 打包 zip（平铺，Compress-Archive）──
Write-Host "[3/4] 打包 zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
Remove-Item -Force $ZipPath -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath
Write-Host "已打包: $ZipPath" -ForegroundColor Green

# ── 4. 分发目标 ─────────────────────────────────────
if ($ToResources) {
    # 随安装包内置分发：复制进 src-tauri/resources/plugins（tauri.conf.json resources 通配）
    $RepoRoot = (Resolve-Path (Join-Path $PluginDir "..\..\..")).Path
    $ResDir = Join-Path $RepoRoot "src-tauri\resources\plugins"
    New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
    # 同 id 旧版本清理（构建残留防多版本并存）
    Get-ChildItem $ResDir -Filter "$PluginId-*.zip" -ErrorAction SilentlyContinue | Remove-Item -Force
    Copy-Item $ZipPath $ResDir
    Write-Host "已复制到内置资源: $ResDir\$PluginId-$Version.zip" -ForegroundColor Green
}

if ($Install) {
    # 安装到本机 VoiceAssist（exe 同级 plugins/），更新 registry.json
    $ExeCandidates = @(
        (Join-Path $Env:LOCALAPPDATA "Programs\VoiceAssist"),
        (Join-Path $Env:LOCALAPPDATA "Programs\TTSassist")
    ) | Where-Object { Test-Path $_ }
    if (-not $ExeCandidates) { throw "未找到 VoiceAssist 安装目录，请手动解压 $ZipPath" }
    $TargetRoot = Join-Path $ExeCandidates[0] "plugins"
    $TargetDir = Join-Path $TargetRoot $PluginId
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
    Copy-Item (Join-Path $StageDir "*") $TargetDir -Force
    Write-Host "已安装到: $TargetDir" -ForegroundColor Green
    Write-Host "提醒：registry.json 需在应用内或手动登记后生效" -ForegroundColor Yellow
}
