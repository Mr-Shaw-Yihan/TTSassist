# MOSS TTS（Mossland）插件 - 打包脚本
#
# 用法（在 plugins/engines/moss-tts 目录）：
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1            # 仅打包 zip
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本地 exe 同级 plugins/
#
# -Install 前先关闭主程序（运行中的 dll 无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "moss-tts"
$PluginName = "MOSS TTS（Mossland）"
$Version    = "0.1.0"
# 与 fish/mimo 同口径取 1.8.0：宿主 1.8.7 起就有通用插件配置与「我的音色」，
# 写 1.9.0 会让当前开发树（版本号在 P6 发布时才 bump）直接拒载、无法真机验收
$MinAppVer  = "1.8.0"
$Desc       = "Mossland MOSS 云端语音合成（非流式，整段 MP3）：需 API Key；不自带音色，请到 Mossland 音色库复制 voice_id 后保存在「我的音色」里选用"

# 通用插件配置声明（宿主 ≥1.8.0 据此渲染设置面板并注入环境变量）。
# env 名必须独立（与其它插件的必填 env 同名会触发宿主 find_required_env_conflict 拒载）。
# 音色库地址只能写在字段说明里：PluginConfigPanel 的 help_url 按钮文案硬编成
# 「获取 API Key ↗」，塞不下第二个链接（要做得给基座加引擎无关的自述位，本版不做）。
# 必须用 [ordered]：PS7(.NET Core) 每进程随机化字符串哈希，普通 hashtable 的
# ConvertTo-Json 键序跨进程不稳定，manifest 就每次不一样 → zip sha 跟着变。
$ConfigDecl = [ordered]@{
    help_url = "https://platform.mosi.cn/app/api-keys"
    fields   = @(
        [ordered]@{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "在 platform.mosi.cn 的「API Key 管理平台」创建；音色 ID 请到 Mossland 音色库 mossland.mosi.cn/voice/library 复制，再保存到下方「我的音色」"
            placeholder = "sk-..."
            env         = "MOSS_TTS_API_KEY"
            required    = $true
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

$DllSrc = Join-Path $WsTarget "target\release\moss_tts.dll"
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
    # 两段式请求（POST 合成 → GET 下载音频），插件内部单请求超时 50 秒；
    # 宿主默认 60 秒会在插件自己报错之前先放弃，而阻塞 FFI 无法取消 → 放宽到 120
    timeout_secs    = 120
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
# 时间戳归一：Compress-Archive 把条目的 LastWriteTime 写进 zip，而 cargo 重链接会
# 刷新 dll 的 mtime——字节一样、zip sha 却变。索引 checksum 记的是 zip 的 sha，
# 于是"没改代码只重跑一次打包"就会让索引和产物漂移（在线安装直接校验失败）。
# 用不带 Z 的字面量：带 Z 会被当 UTC 再换算成本地时间，跨时区构建机写出的
# DOS 时间就不一样了（[datetime] 强转得到 Kind=Unspecified，不做换算）。
$Epoch = [datetime]'2020-01-01T00:00:00'
Get-ChildItem $StageDir | ForEach-Object { $_.LastWriteTime = $Epoch }
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
    if (-not $ExePath) { throw "No exe found (build first). Or set VA_PLUGINS_DIR env var." }
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
