# 插件库一键发布脚本（PowerShell）
#
# 自动发现并打包全部可发布插件（凡含 package.ps1 + dist/package/manifest.json 者），生成官方索引 plugins-index.json，
# 上传到 GitHub Release，并把索引 + 全部 zip 同步到 Gitee dist 镜像分支（国内通道）。
# VoiceAssist 宿主拉索引双通道：
#   主：https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json
#   备：https://gitee.com/yihwan/TTSassist/raw/dist/plugins-index.json
#
# 用法（在 plugins 目录下）：
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1                # 打包 + 生成索引 + 创建/更新 Release
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -Tag plugins-v0.2.0
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -DryRun        # 只打包和生成索引，不碰 GitHub
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -SkipBuild        # 不重新编译，直接用现有 dist 产物重生成索引
#   powershell -ExecutionPolicy Bypass -File .\publish.ps1 -SyncResources  # 同时把 zip 同步进 resources/plugins（供本体安装包内置）
#
# 前置条件：已安装 gh CLI 且已登录（gh auth status 检查）。
#
# ⚠️ 重要：宿主用 releases/latest 定位索引，latest 始终指向最新非 prerelease 的 Release。
#    因此【每次发应用本体新版本时，也要把 plugins-index.json 和全部插件 zip 附到那个 Release】，
#    否则 latest 转移后索引就拉不到了。本脚本的 -AlsoAttach 思路即为此。

param(
    [string]$Tag = "plugins-v0.1.0",
    [switch]$DryRun,
    [switch]$SkipBuild,
    [switch]$SyncResources
)

$ErrorActionPreference = "Stop"

$RepoBase = "https://github.com/Mr-Shaw-Yihan/TTSassist"
$GiteeBase = "https://gitee.com/yihwan/TTSassist"
$GiteeRemote = "gitee"
$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $PluginsDir
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

$ResourcesDir = Join-Path $RepoRoot 'src-tauri\resources\plugins'

# ── 1. 自动发现可发布插件：含 package.ps1 且已产出 dist/package/manifest.json 的目录
#     索引字段（name/version/type/description）全部从各插件 manifest.json 派生（单一权威源），
#     杜绝手工清单漂移（v1.8.2 事故根因：硬编码数组漏了 hojo-tts、lan-remote 名字陈旧）。
$PluginDirs = Get-ChildItem $PluginsDir -Directory | Where-Object {
    (Test-Path (Join-Path $_.FullName 'package.ps1')) -and
    (Test-Path (Join-Path $_.FullName 'dist\package\manifest.json'))
} | Sort-Object Name
if (-not $PluginDirs) { throw "未发现任何可发布插件（plugins/*/dist/package/manifest.json）" }

$Entries = @()
$Assets  = @()
foreach ($dir in $PluginDirs) {
    $id = $dir.Name
    Write-Host ""
    Write-Host "══ $id ══" -ForegroundColor Cyan
    if (-not $SkipBuild) {
        & (Join-Path $dir.FullName 'package.ps1')
        if ($LASTEXITCODE -ne 0 -and $null -ne $LASTEXITCODE) { throw "$id 打包失败" }
    }

    $man = Get-Content (Join-Path $dir.FullName 'dist\package\manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    # zip 名必须等于 <id>-<manifest.version>.zip（隐式断言版本一致）
    $Zip = Get-ChildItem (Join-Path $dir.FullName 'dist') -Filter "$id-$($man.version).zip" |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $Zip) { throw "找不到 $id 的 zip 产物（$id-$($man.version).zip）；如不重新打包请用 -SkipBuild 并确保 dist 已有对应 zip" }

    $Sha = (Get-FileHash $Zip.FullName -Algorithm SHA256).Hash.ToLower()
    $Entries += [ordered]@{
        id           = $man.id
        name         = $man.name
        version      = $man.version
        download_url = "$RepoBase/releases/latest/download/$($Zip.Name)"
        mirror_url   = "$GiteeBase/raw/dist/$($Zip.Name)"
        checksum     = $Sha
        description  = $man.description
        plugin_type  = $man.type
    }
    $Assets += $Zip.FullName

    # 可选：把 dist zip 同步到 resources/plugins（供本体安装包内置），并清理同 id 旧版本
    if ($SyncResources) {
        if (-not (Test-Path $ResourcesDir)) { New-Item -ItemType Directory -Force -Path $ResourcesDir | Out-Null }
        Get-ChildItem $ResourcesDir -Filter "$id-*.zip" | Where-Object { $_.Name -ne $Zip.Name } | Remove-Item -Force
        Copy-Item $Zip.FullName -Destination (Join-Path $ResourcesDir $Zip.Name) -Force
        Write-Host "  -> 已同步 resources/plugins/$($Zip.Name)" -ForegroundColor DarkGray
    }
    Write-Host "$id v$($man.version)  zip SHA-256: $Sha" -ForegroundColor Green
}

# ── 2. 生成官方索引 plugins-index.json ────────────────────
$Index = [ordered]@{ plugins = @($Entries) }
$IndexPath = Join-Path $PluginsDir "plugins-index.json"
[System.IO.File]::WriteAllText($IndexPath, ($Index | ConvertTo-Json -Depth 5), $Utf8NoBom)
Write-Host ""
Write-Host "索引已生成: $IndexPath" -ForegroundColor Green
$Assets += $IndexPath

# ── 2b. 一致性自检：防漂移（对刚生成的索引校验 dist/resources）──────────────
$verifyArgs = @{ IndexPath = $IndexPath }
if (-not $SyncResources) { $verifyArgs['SkipResources'] = $true }
& (Join-Path $PluginsDir 'verify-index.ps1') @verifyArgs
if ($LASTEXITCODE -ne 0) { throw "一致性自检未通过，已中止发布" }

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
# 远程配置（邀请码等）随 dist 分发，宿主经 Gitee raw 读取；单独更新可用 sync-remote-config.ps1
Copy-Item (Join-Path $PluginsDir "remote-config.json") -Destination $DistWork -Force

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
