# 插件库一键发布脚本（PowerShell）
#
# 把全部可发布插件（edge-tts / mimo-asr / genie-tts）打包、生成官方索引 plugins-index.json，
# 上传到 GitHub Release，并把索引 + 全部 zip 同步到 Gitee dist 镜像分支（国内通道）。
# VoiceAssist 宿主拉索引双通道：
#   主：https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json
#   备：https://gitee.com/yihwan/TTSassist/raw/dist/plugins-index.json
#
# 用法（在 plugins 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1                # 打包 + 生成索引 + 创建/更新 Release
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -Tag plugins-v0.2.0
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -DryRun        # 只打包和生成索引，不碰 GitHub
#
# 前置条件：已安装 gh CLI 且已登录（gh auth status 检查）。
#
# ⚠️ 重要：宿主用 releases/latest 定位索引，latest 始终指向最新非 prerelease 的 Release。
#    因此【每次发应用本体新版本时，也要把 plugins-index.json 和全部插件 zip 附到那个 Release】，
#    否则 latest 转移后索引就拉不到了。本脚本的 -AlsoAttach 思路即为此。

param(
    [string]$Tag = "plugins-v0.1.0",
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$RepoBase = "https://github.com/Mr-Shaw-Yihan/TTSassist"
$GiteeBase = "https://gitee.com/yihwan/TTSassist"
$GiteeRemote = "gitee"
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $PluginsDir
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# 可发布插件清单（id / 显示名 / 类型 / 描述），版本与 checksum 由打包产物自动得出
# Type 与各插件 manifest.json 的 type 一致：tts_engine 语音合成 / asr_engine 语音输入（识别）
$Plugins = @(
    @{ Id = "edge-tts"; Name = "Edge TTS（免费·微软）";    Type = "tts_engine"; Desc = "免费、无需 Key 的微软 Edge 语音（非官方接口，可能不稳定）" },
    @{ Id = "mimo-asr"; Name = "MiMo ASR（小米·云端）";    Type = "asr_engine"; Desc = "小米 MiMo-V2.5-ASR 云端语音识别，支持中英文及自动语种检测" },
    @{ Id = "genie-tts"; Name = "Genie TTS（本地·离线）";  Type = "tts_engine"; Desc = "GPT-SoVITS ONNX 本地推理引擎，CPU 离线合成，音色包可扩展（首次使用自动下载运行环境约 1.1GB，每个音色另约 320MB）" }
)

# ── 1. 逐个打包（复用各插件自己的 package.ps1）─────────────
$Entries = @()
$Assets  = @()
foreach ($p in $Plugins) {
    Write-Host ""
    Write-Host "══ 打包 $($p.Id) ══" -ForegroundColor Cyan
    & (Join-Path $PluginsDir "$($p.Id)\package.ps1")
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { throw "$($p.Id) 打包失败" }

    # 从 dist 找产物 zip（名字形如 <id>-<version>.zip）
    $Zip = Get-ChildItem (Join-Path $PluginsDir "$($p.Id)\dist") -Filter "$($p.Id)-*.zip" |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $Zip) { throw "找不到 $($p.Id) 的 zip 产物" }

    $Version = $Zip.BaseName.Substring($p.Id.Length + 1)   # 去掉 "<id>-" 前缀
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
    Write-Host "$($p.Id) v$Version  zip SHA-256: $Sha" -ForegroundColor Green
}

# ── 2. 生成官方索引 plugins-index.json ────────────────────
$Index = [ordered]@{ plugins = @($Entries) }
$IndexPath = Join-Path $PluginsDir "plugins-index.json"
[System.IO.File]::WriteAllText($IndexPath, ($Index | ConvertTo-Json -Depth 5), $Utf8NoBom)
Write-Host ""
Write-Host "索引已生成: $IndexPath" -ForegroundColor Green
$Assets += $IndexPath

if ($DryRun) {
    Write-Host "DryRun 模式：跳过 GitHub 上传。" -ForegroundColor Yellow
    return
}

# ── 3. 创建或更新 GitHub Release 并上传 ───────────────────
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "未找到 gh CLI，请先安装并登录：winget install GitHub.cli; gh auth login"
}

# 注意：gh 在 Release 不存在时会往 stderr 写 "release not found"，
# 在 $ErrorActionPreference = "Stop" 下带重定向会直接中断脚本，
# 所以这里临时降级为 Continue，只看退出码。
$ErrorActionPreference = "Continue"
$null = gh release view $Tag 2>&1 | Out-String
$ReleaseExists = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = "Stop"

if ($ReleaseExists) {
    Write-Host "Release $Tag 已存在，覆盖上传资产..." -ForegroundColor Yellow
    gh release upload $Tag @Assets --clobber 2>&1 | ForEach-Object { "$_" }
} else {
    Write-Host "创建 Release $Tag ..." -ForegroundColor Cyan
    gh release create $Tag @Assets --title "插件库 $Tag" --notes "VoiceAssist 官方插件索引与各插件安装包。宿主应用从 releases/latest/download/plugins-index.json 拉取索引。" 2>&1 | ForEach-Object { "$_" }
}
if ($LASTEXITCODE -ne 0) { throw "gh release 操作失败" }

Write-Host ""
Write-Host "发布完成 ✅" -ForegroundColor Green
Write-Host "索引地址: $RepoBase/releases/latest/download/plugins-index.json"
Write-Host "提醒：若该 Release 不是最新 Release（后面又发了应用本体），"
Write-Host "      记得把 plugins-index.json 和全部插件 zip 也附到最新的本体 Release 上。"

# ── 4. 同步 Gitee dist 镜像分支（国内通道，GitHub 不可达时宿主自动回退）──
# dist 是无历史的 orphan 分支，只放索引 + zip，每次发布重建（force push），体积恒定。
Write-Host ""
Write-Host "══ 同步 Gitee dist 镜像分支 ══" -ForegroundColor Cyan
$DistWork = Join-Path $env:TEMP "va-dist-push"
if (Test-Path $DistWork) { Remove-Item $DistWork -Recurse -Force }
New-Item -ItemType Directory -Path $DistWork | Out-Null
foreach ($a in $Assets) { Copy-Item $a -Destination $DistWork -Force }

Push-Location $RepoRoot
$OriginBranch = (git rev-parse --abbrev-ref HEAD 2>&1 | Out-String).Trim()
try {
    git checkout --orphan dist-tmp 2>&1 | Out-Null
    # 物理清掉 tracked 文件（node_modules/target 等 ignored 目录不受影响）
    git rm -rf --quiet . 2>&1 | Out-Null
    Get-ChildItem $DistWork -File | Copy-Item -Destination . -Force
    git add (Get-ChildItem $DistWork -File).Name
    git commit -m "dist: 插件分发镜像（索引 + zip，供国内 Gitee 通道下载）" --quiet
    # git push 的 stderr（remote 信息）在 ErrorAction Stop 下会中断脚本，降级只看退出码
    $ErrorActionPreference = "Continue"
    git push $GiteeRemote dist-tmp:dist --force 2>&1 | ForEach-Object { "$_" }
    $PushOk = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = "Stop"
    if (-not $PushOk) { throw "推送 dist 分支到 Gitee 失败" }
} finally {
    git checkout $OriginBranch --quiet 2>&1 | Out-Null
    git branch -D dist-tmp --quiet 2>&1 | Out-Null
    Pop-Location
    Remove-Item $DistWork -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "Gitee dist 镜像已同步 ✅  索引镜像: $GiteeBase/raw/dist/plugins-index.json" -ForegroundColor Green
