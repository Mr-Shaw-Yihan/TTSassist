# 生成并分发「本体应用内升级清单」app-version.json —— 软件内"国内优先升级"的数据源。
#
# 用法示例：
#   powershell -ExecutionPolicy Bypass -File make-app-version.ps1 -Version 1.8.5
#   ... -NotesPath release-notes.md        # 说明来源（缺省自动取仓库根 release-notes.md，即建 Release 用的那份）
#   ... -SetupExe <path>                   # 安装包不在默认 bundle 目录时显式指定
#   ... -SkipGiteeRelease / -DryRun
#
# 前置：gh 已登录；GITEE_TOKEN 环境变量；安装包已构建（npm run tauri build）。
#
# 做四件事，缺任一条软件内的通道就会失效：
#   1. 定位安装包并算 SHA-256 / 字节数
#   2. Gitee 建本体 Release（tag vX.Y.Z）+ 上传安装包附件
#      —— Gitee raw 对 exe 返回 403，安装包只能走 Release 附件直链
#   3. 清单作为资产附到 GitHub 本体 Release（releases/latest/download/app-version.json）
#   4. 清单写进 Gitee dist 分支（客户端主通道 raw）
#      注：不往 origin 推 dist 分支——dist 里存着历史安装包，而 GitHub 已有这些资产，
#      推过去等于同一堆字节再塞一份进 git 存储；客户端的 GitHub 兜底走 Release 资产。
#
# 注：PS 5.1 的 ConvertTo-Json 会把中文转义成 \uXXXX，这是合法 JSON，客户端 serde_json 正常
#     解码，不必为此改写。清单文件本身必须以 UTF-8 无 BOM 写出。
# 本文件须保持 UTF-8 带 BOM（含中文注释）。

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$NotesPath,
    [string]$SetupExe,
    [switch]$SkipGiteeRelease,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $RepoRoot 'gitee-release.ps1')

function Info($m) { Write-Host "[app-ver] $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "[app-ver] $m" -ForegroundColor Yellow }
function Ok($m)   { Write-Host "[app-ver] $m" -ForegroundColor Green }

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw "版本号格式应为 X.Y.Z，实得：$Version" }

$Tag      = "v$Version"
$FileName = "TTSassist_${Version}_x64-setup.exe"

Push-Location $RepoRoot
try {
    $giteeUrl  = (git remote get-url gitee 2>&1 | Out-String).Trim()
    if (-not $giteeUrl -or $giteeUrl -match 'error|没有') { throw "未配置 gitee remote" }
    $originUrl = (git remote get-url origin 2>&1 | Out-String).Trim()
    if (-not $originUrl) { throw "未配置 origin remote" }
    $gt = Parse-OwnerRepo $giteeUrl
    $gh = Parse-OwnerRepo $originUrl
    $ApiBase     = "https://gitee.com/api/v5/repos/$($gt.Owner)/$($gt.Repo)"
    $GhOwnerRepo = "$($gh.Owner)/$($gh.Repo)"

    Info "════════ 本体升级清单  版本=$Version  Tag=$Tag ════════"
    if ($DryRun) { Warn "*** DRY-RUN：所有写/上传/推送仅打印 ***" }

    # ── 1. 安装包与校验值 ──
    if (-not $SetupExe) { $SetupExe = Join-Path $RepoRoot "src-tauri\target\release\bundle\nsis\$FileName" }
    if (-not (Test-Path $SetupExe)) { throw "找不到安装包：$SetupExe（先 npm run tauri build，或显式传 -SetupExe）" }
    $item = Get-Item $SetupExe
    if ($item.Name -ne $FileName) { throw "安装包文件名应为 $FileName，实得 $($item.Name)" }
    $sha  = (Get-FileHash -Path $SetupExe -Algorithm SHA256).Hash.ToLower()
    $size = [int64]$item.Length
    Ok "安装包 $($item.Name)：$size 字节  sha256=$sha"

    # ── 2. 更新说明 ──
    # 默认读仓库里的 release-notes.md（就是建 Release 时 --notes-file 用的那份，字节可控）。
    # 绝不默认从 gh 的标准输出取正文：PS 5.1 会按控制台代码页（GBK）解码 gh 的 UTF-8 输出，
    # 中文被二次编码写进清单、客户端就显示乱码（v1.8.6 踩过）。确需回读时必须先切 UTF-8。
    if (-not $NotesPath -and (Test-Path (Join-Path $RepoRoot 'release-notes.md'))) {
        $NotesPath = 'release-notes.md'
    }
    $notes = ""
    if ($NotesPath) {
        $np = if ([System.IO.Path]::IsPathRooted($NotesPath)) { $NotesPath } else { Join-Path $RepoRoot $NotesPath }
        if (-not (Test-Path $np)) { throw "说明文件不存在：$np" }
        $notes = [System.IO.File]::ReadAllText($np, [System.Text.Encoding]::UTF8)
        Info "更新说明取自 $np"
    } else {
        $prevEnc = [Console]::OutputEncoding
        try {
            [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
            $ErrorActionPreference = "Continue"
            $notes = (& gh release view $Tag --json body --jq .body 2>&1 | Out-String)
            $ghExit = $LASTEXITCODE
            $ErrorActionPreference = "Stop"
        } finally {
            [Console]::OutputEncoding = $prevEnc
        }
        if ($ghExit -ne 0) { throw "取 GitHub Release $Tag 正文失败，请改用 -NotesPath 显式指定：$notes" }
        Info "更新说明回读自 GitHub Release $Tag"
    }
    # 拦网：本项目说明必含中文，一旦一个中文都没有，基本就是取回链路发生了重编码
    if ($notes.Length -gt 0 -and $notes -notmatch '[\u4e00-\u9fff]') {
        Warn "更新说明里没有任何中文字符，请核实取回链路是否发生了重编码（PS 按 GBK 解码 gh 输出即会如此）"
    }

    $giteeDl  = "https://gitee.com/$($gt.Owner)/$($gt.Repo)/releases/download/$Tag/$FileName"
    $githubDl = "https://github.com/$GhOwnerRepo/releases/download/$Tag/$FileName"

    $verObj = [ordered]@{
        version    = $Version
        file       = $FileName
        size       = $size
        sha256     = $sha
        notes      = $notes
        gitee_url  = $giteeDl
        github_url = $githubDl
    }
    $json = $verObj | ConvertTo-Json -Depth 3

    # 资产名必须就叫 app-version.json（gh upload 用文件名作资产名），故放进专用目录
    $manifestDir = Join-Path $env:TEMP "va-appver-$Version"
    if (-not (Test-Path $manifestDir)) { New-Item -ItemType Directory -Path $manifestDir | Out-Null }
    $localManifest = Join-Path $manifestDir 'app-version.json'
    [System.IO.File]::WriteAllText($localManifest, $json, (New-Object System.Text.UTF8Encoding($false)))
    Info "清单已生成：$localManifest（说明 $($notes.Length) 字符）"

    # ── 3. Gitee 本体 Release + 安装包附件 ──
    $token = $env:GITEE_TOKEN
    if ($SkipGiteeRelease) {
        Warn "已跳过 Gitee Release 步骤（-SkipGiteeRelease）"
    } elseif (-not $token) {
        Warn "未设置 GITEE_TOKEN，跳过 Gitee Release/附件。⚠ 不补这一步，软件内的 Gitee 优先下载会 404。"
    } elseif ($DryRun) {
        Warn "[dry-run] Gitee: 建 Release $Tag + 上传附件 $FileName"
    } else {
        $relId = Get-GiteeReleaseIdByTag -ApiBase $ApiBase -Token $token -Tag $Tag
        if (-not $relId) {
            Info "创建 Gitee Release $Tag ..."
            $relId = New-GiteeRelease -ApiBase $ApiBase -Token $token -Tag $Tag `
                -Name "电子声带 TTSassist $Version" -Body $notes
        } else {
            Info "Gitee Release $Tag 已存在（id=$relId）"
        }
        $did = Add-GiteeAttachmentIfMissing -ApiBase $ApiBase -Token $token -ReleaseId $relId -FilePath $SetupExe
        if ($did) { Ok "Gitee 附件已上传：$FileName" }
        else { Warn "Gitee 附件已存在 → 跳过上传（如需替换，先在网页删除该附件再重跑）" }
        Info "Gitee API 读接口有缓存，请以 Release 页面为准确认附件可见。"
    }

    # ── 4. 清单附到 GitHub 本体 Release ──
    if ($DryRun) {
        Warn "[dry-run] gh release upload $Tag app-version.json --clobber"
    } else {
        $ErrorActionPreference = "Continue"
        & gh release upload $Tag $localManifest --clobber 2>&1 | ForEach-Object { "$_" }
        $upExit = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($upExit -ne 0) { throw "gh release upload 失败（清单未附到 $Tag）" }
        Ok "清单已作为资产附到 GitHub Release $Tag"
    }

    # ── 5. dist 分支（Gitee raw 主入口）──
    if ($DryRun) {
        Warn "[dry-run] 将把 app-version.json 提交进 Gitee dist 分支并推送"
    } else {
        $Tmp = Join-Path $env:TEMP "va-dist-appver-body"
        if (Test-Path $Tmp) { Remove-Item $Tmp -Recurse -Force }
        $ErrorActionPreference = "Continue"
        git clone --depth 1 --branch dist $giteeUrl $Tmp 2>&1 | ForEach-Object { "$_" }
        $cloneOk = ($LASTEXITCODE -eq 0)
        $ErrorActionPreference = "Stop"
        if (-not $cloneOk) { throw "克隆 Gitee dist 分支失败" }
        try {
            Copy-Item $localManifest (Join-Path $Tmp 'app-version.json') -Force
            Push-Location $Tmp
            git add app-version.json
            git commit -m "dist: 本体应用内升级清单 → $Version" --quiet 2>&1 | ForEach-Object { "$_" }
            $commitExit = $LASTEXITCODE
            $ErrorActionPreference = "Continue"
            git push origin HEAD:refs/heads/dist 2>&1 | ForEach-Object { "$_" }
            $pushGiteeOk = ($LASTEXITCODE -eq 0)
            Pop-Location
            $ErrorActionPreference = "Stop"
            # 清单与上一版一致时无改动，不是错误
            if ($commitExit -ne 0) { Warn "dist 无改动（清单与线上一致），本次未产生新提交" }
            if (-not $pushGiteeOk) { throw "推送 Gitee dist 分支失败" }
            Ok "清单已进 dist 分支（Gitee raw）"
        } finally {
            Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    Ok "════════ 完成 ════════"
    Info "客户端读取顺序：https://gitee.com/$($gt.Owner)/$($gt.Repo)/raw/dist/app-version.json → https://github.com/$GhOwnerRepo/releases/latest/download/app-version.json"
    Info "发布后抽查两处 raw 均 200。比对两份清单请用字段级而非逐字节：Gitee 入库会把 CRLF 归一化成 LF。"
    Info "安装包则必须以 SHA-256 为准（两端附件应同为一个本地文件，字节一致）。"
}
finally {
    Pop-Location
}
