# MiMo ASR 插件打包脚本（PowerShell）
#
# 用法（在 plugins/mimo-asr 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\package.ps1            # 只打包 zip
#   powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本机 VoiceAssist
#
# -Install 会装进 <exe同级>/plugins/mimo-asr/（阶段 22 起脱离 AppData）并更新 registry.json。
# 安装前请先关闭 VoiceAssist（运行中的 dll 被占用无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "mimo-asr"
$PluginName = "MiMo ASR（小米·云端）"
$Version    = "0.1.1"
$MinAppVer  = "1.5.0"
$Desc       = "小米 MiMo-V2.5-ASR 云端语音识别，支持中英文及自动语种检测"

# 通用插件配置声明（宿主 ≥1.8.0 按 manifest 渲染设置面板并注入环境变量）。
# 背景：v0.1.0 的 MIMO_API_KEY 靠宿主 lib.rs 从顶层 mimo_api_key 直接 set_var，
# manifest 里没有任何 config 声明；宿主删掉那条注入后必须改走本机制，否则 ASR 失 Key。
# min_app_version 不必提高：老宿主仍走顶层注入，新宿主走 config 注入，两条路都通。
# 必须用 [ordered]：PS7(.NET Core) 每进程随机化字符串哈希，普通 hashtable 的
# ConvertTo-Json 键序跨进程不稳定 → manifest 每次不一样 → zip sha 跟着漂移，
# 索引 checksum 与产物就对不上（与 mimo-tts 同机制，见那边同款注释）。
$ConfigDecl = [ordered]@{
    help_url = "https://platform.xiaomimimo.com?ref=5P9J2B"
    fields   = @(
        [ordered]@{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "从小米 MiMo 开放平台获取（与 MiMo TTS 用的是同一个 Key）"
            env         = "MIMO_API_KEY"
            required    = $true
        }
    )
}

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

$DllSrc = Join-Path $WsTarget "target\release\mimo_asr.dll"
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
    type            = "asr_engine"
    platform        = @("windows")
    entry           = "plugin.dll"
    min_app_version = $MinAppVer
    checksum        = $Hash
    description     = $Desc
    category        = "remote"
    timeout_secs    = 120
    config          = $ConfigDecl
}
# 注意：必须用无 BOM 写入；必须 -Depth 6（默认 Depth=2 会把 config.fields 的嵌套结构截成字符串）
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 6),
    $Utf8NoBom
)

# ── 3. 压缩 zip ──────────────────────────────────────
Write-Host "[3/4] 打包 zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
# 时间戳归一：Compress-Archive 把条目 LastWriteTime 写进 zip，而 cargo 重链接会刷新 dll 的
# mtime——字节一样、zip sha 却变。索引 checksum 记的是 zip 的 sha，于是不改代码只重跑一次
# 打包就会让索引与产物漂移（在线安装直接校验失败）。固定成同一时刻后同产物必得同 zip。
# 用不带 Z 的字面量：带 Z 会被当 UTC 再换算成本地时间，跨时区构建机写出的 DOS 时间就不一样了。
$Epoch = [datetime]'2020-01-01T00:00:00'
Get-ChildItem $StageDir | ForEach-Object { $_.LastWriteTime = $Epoch }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -Force
Write-Host "打包完成: $ZipPath" -ForegroundColor Green
Write-Host "  plugin.dll SHA-256: $Hash"
Write-Host ("  索引 checksum 应用 zip SHA-256: {0}" -f (Get-FileHash $ZipPath -Algorithm SHA256).Hash.ToLower()) -ForegroundColor Yellow

# 同步到安装包资源目录（tauri build 时内置进安装包，供"插件库"安装）
$ResDir = Join-Path $PluginDir "..\..\..\src-tauri\resources\plugins"
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
    $RepoRoot   = (Resolve-Path (Join-Path $PluginDir "..\..\..")).Path
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
    Write-Host "启动 VoiceAssist 后，ASR 插件将自动加载。"
}
