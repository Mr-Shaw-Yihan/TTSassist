# 插件库一键发布脚本（PowerShell）
#
# 把全部可发布插件（edge-tts / mimo-asr）打包、生成官方索引 plugins-index.json，
# 并上传到 GitHub Release。VoiceAssist 宿主从以下地址拉取索引：
#   https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json
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
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# 可发布插件清单（id / 显示名 / 描述），版本与 checksum 由打包产物自动得出
$Plugins = @(
    @{ Id = "edge-tts"; Name = "Edge TTS（免费·微软）";    Desc = "免费、无需 Key 的微软 Edge 语音（非官方接口，可能不稳定）" },
    @{ Id = "mimo-asr"; Name = "MiMo ASR（小米·云端）";    Desc = "小米 MiMo-V2.5-ASR 云端语音识别，支持中英文及自动语种检测" }
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
        checksum     = $Sha
        description  = $p.Desc
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

$Existing = gh release view $Tag 2>$null
if ($LASTEXITCODE -eq 0) {
    Write-Host "Release $Tag 已存在，覆盖上传资产..." -ForegroundColor Yellow
    gh release upload $Tag @Assets --clobber
} else {
    Write-Host "创建 Release $Tag ..." -ForegroundColor Cyan
    gh release create $Tag @Assets --title "插件库 $Tag" --notes "VoiceAssist 官方插件索引与各插件安装包。宿主应用从 releases/latest/download/plugins-index.json 拉取索引。"
}
if ($LASTEXITCODE -ne 0) { throw "gh release 操作失败" }

Write-Host ""
Write-Host "发布完成 ✅" -ForegroundColor Green
Write-Host "索引地址: $RepoBase/releases/latest/download/plugins-index.json"
Write-Host "提醒：若该 Release 不是最新 Release（后面又发了应用本体），"
Write-Host "      记得把 plugins-index.json 和全部插件 zip 也附到最新的本体 Release 上。"
