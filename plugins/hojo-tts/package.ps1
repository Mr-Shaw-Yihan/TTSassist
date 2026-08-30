# Hojo-TTS-Light-80M 插件打包脚本（PowerShell）
#
# 用法（在 plugins/hojo-tts 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 只打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本机 VoiceAssist
#
# -Install 会装进 <exe同级>/plugins/hojo-tts/ 并更新 registry.json（阶段 22 起脱离 APPDATA）。
# 安装前请先关闭 VoiceAssist（运行中的 dll 被占用无法覆盖）。
#
# 注意：zip 只含 manifest.json + plugin.dll 两个文件。Python 运行时、ONNX 模型、
# 音色参考音频都不随包分发——由插件首次使用时自动下载到数据目录（合计约 1GB）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "hojo-tts"
$PluginName = "Hojo TTS（本地·离线）"
$Version    = "0.1.0"
$MinAppVer  = "1.4.0"
$Desc       = "Hojo-TTS-Light-80M 零样本音色克隆本地推理引擎，CPU 离线合成，中英混读，音色包可扩展（首次使用自动下载运行环境与模型）"
# 资源需求说明：供用户在下载安装运行环境前判断本机配置是否够用
$Requirements = "首次使用需联网下载 Python 运行环境与依赖约 1.2GB、语音模型约 460MB，合计约 1.7GB；本机 CPU 推理（无需显卡），运行时内存占用约 2-3GB，建议内存 8GB 以上、磁盘预留 3GB。"

# 说明：面向大众用户，不声明 manifest config（下载源/安装源对小白无意义且徒增困惑）。
# 网络容错全部内置于插件：模型下载多端点回退（hf-mirror → 官方），pip 安装多源
# 回退（清华 → 腾讯 → 官方 PyPI）；高级用户可用环境变量 HOJO_TTS_HF_ENDPOINT /
# HOJO_TTS_PIP_INDEX_URL 覆盖（无 UI，排障后门，见 src/paths.rs）。

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

# crate 名 hojo-tts → dll 文件名连字符变下划线
$DllSrc = Join-Path $PluginDir "target\release\hojo_tts.dll"
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
    category        = "local"
    timeout_secs    = 1200
    requirements    = $Requirements
}
# 注意：必须用无 BOM 写入（Set-Content -Encoding UTF8 在 PS 5.1 会带 BOM，宿主的 JSON 解析器不认）
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 6),
    $Utf8NoBom
)

# ── 3. 压缩 zip ──────────────────────────────────────
Write-Host "[3/4] 打包 zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -Force
Write-Host "打包完成: $ZipPath" -ForegroundColor Green
Write-Host "  plugin.dll SHA-256: $Hash"

# 同步到安装包资源目录（tauri build 时内置进安装包，供"插件库"安装）
$ResDir = Join-Path $PluginDir "..\..\src-tauri\resources\plugins"
New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
Remove-Item (Join-Path $ResDir "$PluginId-*.zip") -ErrorAction SilentlyContinue
Copy-Item $ZipPath $ResDir -Force
Write-Host "已同步到安装包资源: $ResDir" -ForegroundColor Green

# ── 4. 可选：安装到本机 VoiceAssist ──────────────────
if ($Install) {
    Write-Host "[4/4] 安装到本机 VoiceAssist..." -ForegroundColor Cyan

    # 检查应用是否在运行（dll 占用会导致覆盖失败）
    if (Get-Process -Name "TTSassist", "voiceassist" -ErrorAction SilentlyContinue) {
        throw "VoiceAssist 正在运行，请先关闭再执行 -Install"
    }

    # 阶段 22：插件装在 exe 同级 plugins/ 目录（脱离 APPDATA 系统盘）
    # 自动探测 exe 位置：优先 release；release 下两个名字都在（tauri build 会复制出
    # productName 命名的 TTSassist.exe），debug 只有 crate 名 voiceassist.exe
    $RepoRoot   = (Resolve-Path (Join-Path $PluginDir "..\..")).Path
    $ExePaths   = @(
        (Join-Path $RepoRoot "src-tauri\target\release\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\release\voiceassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\voiceassist.exe")
    )
    $ExePath = $ExePaths | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $ExePath) {
        throw "找不到 TTSassist.exe（请先 cargo build）。也可设置 VA_PLUGINS_DIR 环境变量指定插件目录。"
    }
    # 支持环境变量覆盖（与宿主 resolve_plugins_root 逻辑一致）
    if ($env:VA_PLUGINS_DIR) {
        $PluginsDir = $env:VA_PLUGINS_DIR
    } else {
        $PluginsDir = Join-Path (Split-Path -Parent $ExePath) "plugins"
    }
    $TargetDir  = Join-Path $PluginsDir $PluginId
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
    Write-Host "提示：首次合成会自动下载运行环境与模型（合计约 1.7GB），请耐心等待。"
}
