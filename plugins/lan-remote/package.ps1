# lan-remote 插件打包脚本（PowerShell）
#
# 用法（在 plugins/lan-remote 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 只打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本机 VoiceAssist
#
# -Install 会装进 <exe同级>/plugins/lan-remote/ 并更新 registry.json。
# 安装前请先关闭 VoiceAssist（运行中的 dll 被占用无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "lan-remote"
$PluginName = "手机遥控（局域网）"
$Version    = "0.1.0"
$MinAppVer  = "1.7.9"
$Desc       = "手机遥控 PC 端：局域网 WebSocket 服务 + mDNS 发现 + 配对码配对，需配合移动端 App 使用"

$PluginDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$DistDir = Join-Path $PluginDir "dist"
$StageDir = Join-Path $DistDir "package"
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

$DllSrc = Join-Path $PluginDir "target\release\lan_remote.dll"
if (-not (Test-Path $DllSrc)) { throw "找不到构建产物: $DllSrc" }

# ── 2. 暂存目录（zip 内容：平铺 manifest.json + plugin.dll）──
Write-Host "[2/4] 生成 manifest.json（含 SHA-256）..." -ForegroundColor Cyan
Remove-Item -Recurse -Force $DistDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item $DllSrc (Join-Path $StageDir "plugin.dll")

$Hash = (Get-FileHash (Join-Path $StageDir "plugin.dll") -Algorithm SHA256).Hash.ToLower()
$Manifest = [ordered]@{
    id                   = $PluginId
    name                 = $PluginName
    version              = $Version
    type                 = "service"
    platform             = @("windows")
    entry                = "plugin.dll"
    min_app_version      = $MinAppVer
    checksum             = $Hash
    description          = $Desc
    category             = "remote"
    requires_host_bridge = $true
    config               = @{
        fields = @(
            @{
                key         = "pair_code"
                type        = "display"
                label       = "配对码"
                env         = "LAN_REMOTE_PAIR_CODE"
                description = "手机 App 输入此 6 位码完成配对；配对成功或刷新后自动更换"
            }
            @{
                key         = "host_addr"
                type        = "display"
                label       = "遥控地址"
                env         = "LAN_REMOTE_HOST_ADDR"
                description = "手机无法自动发现 PC 时，在 App 手动填入此地址连接"
            }
        )
    }
}
# 注意：必须用无 BOM 写入
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 5),
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

    # 自动探测 exe 位置（与其它插件脚本一致：release 优先）
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
    Write-Host "启动 VoiceAssist 后服务插件将自动加载（首次会弹 Windows 防火墙授权，必须允许）。"
}
