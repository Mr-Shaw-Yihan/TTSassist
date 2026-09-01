# 遥控 App（remote-app/，Flutter）一键发布脚本
# 覆盖 发布流程.md §七「App 单独发版三步」：构建 APK → 双平台 Release 附件 → 更新 dist 版本清单。
#
# 用法示例：
#   # 完整发布（版本读自 pubspec.yaml，构建 + 上传 + 更新 version.json）
#   powershell -ExecutionPolicy Bypass -File remote-app\tools\publish_remote_app.ps1 -Notes "本次更新说明"
#   # 指定版本并预演（不执行任何写/上传操作，仅打印计划）
#   ... -Version 1.8.3 -Notes "..." -DryRun
#   # 复用已构建 APK，只补推 GitHub
#   ... -ApkPath <apk> -SkipGitee -SkipDist
#   # 只刷新 dist 版本清单（Release 附件已在双平台就位）
#   ... -SkipBuild -SkipGitHub -SkipGitee
#
# 前置条件：
#   - gh CLI 已 `gh auth login`（GitHub 通道）
#   - 环境变量 GITEE_TOKEN（Gitee 私人令牌，需项目写权限；仅 Gitee 步骤用到，缺省则跳过 Gitee 并告警）
#   - flutter / git 在 PATH；Windows 自带 curl.exe（Gitee Release 附件 multipart 上传用）
#   - 版本单一源 = pubspec.yaml：先手改 version: X.Y.Z+N（每次重打包 +N 递增），再跑本脚本
#
# ⚠️ latest 语义铁律：GitHub 的 remote-vX.Y.Z 一律标 --prerelease，
#    否则抢走 releases/latest，导致宿主拉不到 plugins-index.json（404，插件在线安装失效）。

[CmdletBinding()]
param(
    [string]$Version,                       # X.Y.Z；缺省则从 remote-app/pubspec.yaml 解析
    [string]$Notes = "",                     # 更新说明（写进 Release notes 与 version.json）
    [string]$ApkPath,                        # 直接指定已构建好的 APK（隐含 -SkipBuild）
    [switch]$SkipBuild,
    [switch]$SkipGitHub,
    [switch]$SkipGitee,
    [switch]$SkipDist,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Info($m) { Write-Host "[publish-remote] $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "[publish-remote] $m" -ForegroundColor Yellow }
function Ok($m)   { Write-Host "[publish-remote] $m" -ForegroundColor Green }

# ── 目录定位：脚本在 remote-app/tools/ 下 → 上两级为仓库根 ──
$ToolsDir     = Split-Path -Parent $MyInvocation.MyCommand.Path
$RemoteAppDir = Split-Path -Parent $ToolsDir
$RepoRoot     = Split-Path -Parent $RemoteAppDir

function ParseOwnerRepo([string]$url) {
    $n = ($url -replace '\.git$', '' -replace '^[a-z]+://[^/]+/', '' -replace '^[^@]+@[^:]+:', '')
    $p = $n.Split('/')
    if ($p.Count -lt 2) { throw "无法从远端 URL 解析 owner/repo：$url" }
    return @{ Owner = $p[-2]; Repo = $p[-1] }
}

Push-Location $RepoRoot
try {
    # ── 远端 ──
    $giteeUrl = (git remote get-url gitee 2>&1 | Out-String).Trim()
    if (-not $giteeUrl -or $giteeUrl -match 'error|没有') { throw "未配置 gitee remote" }
    $originUrl = (git remote get-url origin 2>&1 | Out-String).Trim()
    if (-not $originUrl) { throw "未配置 origin remote" }
    $gt = ParseOwnerRepo $giteeUrl
    $gh = ParseOwnerRepo $originUrl

    # ── 版本：显式 > pubspec 解析 ──
    if (-not $Version) {
        $pub = Join-Path $RemoteAppDir 'pubspec.yaml'
        $line = (Select-String -Path $pub -Pattern '^version:' | Select-Object -First 1).Line
        # version: X.Y.Z+N  → 取 X.Y.Z
        if ($line -match 'version:\s*([0-9]+\.[0-9]+\.[0-9]+)') { $Version = $Matches[1] }
        else { throw "无法从 pubspec.yaml 解析版本号，请显式传 -Version" }
        Info "从 pubspec.yaml 解析版本：$Version"
    }
    if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw "版本号格式应为 X.Y.Z，实得：$Version" }

    $Tag      = "remote-v$Version"
    $ApkName  = "voiceassist-remote-$Version-arm64.apk"
    $githubDl = "https://github.com/$($gh.Owner)/$($gh.Repo)/releases/download/$Tag/$ApkName"
    $giteeDl  = "https://gitee.com/$($gt.Owner)/$($gt.Repo)/releases/download/$Tag/$ApkName"

    # ── 令牌可用性预判（Gitee）──
    $token = $env:GITEE_TOKEN
    if (-not $SkipGitee -and -not $token) {
        Warn "未设置环境变量 GITEE_TOKEN，自动跳过 Gitee Release 附件步骤（dist 清单里的 gitee_url 仍会写，需你稍后手动补附件）。"
        $SkipGitee = $true
    }

    Info "══════════════════════════════════════════════"
    Info "遥控 App 发布  版本=$Version  Tag=$Tag"
    Info "  GitHub = $($gh.Owner)/$($gh.Repo)"
    Info "  Gitee  = $($gt.Owner)/$($gt.Repo)"
    Info "  跳过：$(('构建:{0} GitHub:{1} Gitee:{2} dist:{3}' -f $SkipBuild,$SkipGitHub,$SkipGitee,$SkipDist))"
    if ($DryRun) { Warn "*** DRY-RUN：以下所有写/上传/推送操作仅打印，不执行 ***" }
    Info "══════════════════════════════════════════════"

    # ══ 步骤 1：构建并抽取 arm64 APK ══
    $stageDir = Join-Path $RemoteAppDir 'build\publish-staging'
    $stageApk = Join-Path $stageDir $ApkName
    if ($ApkPath) {
        if (-not (Test-Path $ApkPath)) { throw "指定的 APK 不存在：$ApkPath" }
        $SkipBuild = $true
        if (-not (Test-Path $stageDir)) { New-Item -ItemType Directory -Path $stageDir | Out-Null }
        Copy-Item $ApkPath $stageApk -Force
        Ok "已使用外部 APK → $stageApk"
    }
    if (-not $SkipBuild) {
        if ($DryRun) {
            Warn "[dry-run] 跳过构建：flutter build apk --release --split-per-abi --build-name=$Version"
            # 预演需要一个占位 APK 路径，供后续步骤仅打印
            if (-not (Test-Path $stageApk)) { $found = $null }
        } else {
            Push-Location $RemoteAppDir
            Info "flutter build apk --release --split-per-abi --build-name=$Version ..."
            & flutter build apk --release --split-per-abi --build-name=$Version 2>&1 | ForEach-Object { "$_" }
            $buildExit = $LASTEXITCODE
            Pop-Location
            if ($buildExit -ne 0) { throw "flutter build 失败（退出码 $buildExit）" }
        }
    }
    if (-not $DryRun) {
        $found = Get-ChildItem -Path (Join-Path $RemoteAppDir 'build\app\outputs') -Recurse -Filter 'app-arm64-v8a-release.apk' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $ApkPath) {
            if (-not $found) { throw "构建产物中找不到 app-arm64-v8a-release.apk（确认 --split-per-abi 是否生成 arm64-v8a）" }
            if (-not (Test-Path $stageDir)) { New-Item -ItemType Directory -Path $stageDir | Out-Null }
            Copy-Item $found.FullName $stageApk -Force
        }
    }
    if (-not $DryRun -and -not (Test-Path $stageApk)) { throw "暂存 APK 缺失：$stageApk" }
    if (-not $DryRun) { Ok "待发布 APK：$stageApk（$([math]::Round((Get-Item $stageApk).Length/1MB,1)) MB）" }

    # ══ 步骤 2A：GitHub Release（gh，强制 prerelease）══
    if (-not $SkipGitHub) {
        if ($DryRun) {
            Warn "[dry-run] gh release $Tag ← $ApkName（--prerelease）"
        } else {
            $ErrorActionPreference = "Continue"
            $null = gh release view $Tag 2>&1 | Out-String
            $exists = ($LASTEXITCODE -eq 0)
            $ErrorActionPreference = "Stop"
            if ($exists) {
                Info "GitHub Release $Tag 已存在 → 覆盖上传附件"
                & gh release upload $Tag $stageApk --clobber 2>&1 | ForEach-Object { "$_" }
                if ($LASTEXITCODE -ne 0) { throw "gh release upload 失败" }
            } else {
                Info "创建 GitHub Release $Tag（prerelease）..."
                & gh release create $Tag $stageApk --target main --prerelease --title "遥控 App $Version" --notes $Notes 2>&1 | ForEach-Object { "$_" }
                if ($LASTEXITCODE -ne 0) { throw "gh release create 失败" }
            }
            # 兜底：无论新建/已存在，再次强制 prerelease（防手滑把 latest 抢走）
            & gh release edit $Tag --prerelease 2>&1 | ForEach-Object { "$_" }
            if ($LASTEXITCODE -ne 0) { Warn "gh release edit --prerelease 未成功，请手动确认 $Tag 为 prerelease！" }
            Ok "GitHub 附件就位：$githubDl"
        }
    }

    # ══ 步骤 2B：Gitee Release 附件（API + curl multipart）══
    if (-not $SkipGitee) {
        $apiBase = "https://gitee.com/api/v5/repos/$($gt.Owner)/$($gt.Repo)"
        if ($DryRun) {
            Warn "[dry-run] Gitee: 建 Release $Tag + 上传 $ApkName 附件（access_token 走 GITEE_TOKEN）"
        } else {
            # 查 tag 是否已有 Release
            $relId = $null
            try {
                $resp = & curl.exe -s "$apiBase/releases/tags/$Tag`?access_token=$token"
                $obj = $resp | ConvertFrom-Json
                if ($obj -and $obj.id) { $relId = $obj.id }
            } catch { }

            if (-not $relId) {
                Info "创建 Gitee Release $Tag ..."
                $resp = & curl.exe -s -X POST "$apiBase/releases" `
                    --data-urlencode "access_token=$token" `
                    --data-urlencode "tag_name=$Tag" `
                    --data-urlencode "name=遥控 App $Version" `
                    --data-urlencode "body=$Notes" `
                    --data-urlencode "target_commitish=main"
                $obj = $resp | ConvertFrom-Json
                if (-not $obj -or -not $obj.id) { throw "Gitee 建 Release 失败：$resp" }
                $relId = $obj.id
            } else {
                Info "Gitee Release 已存在（id=$relId），检查附件..."
            }

            # 查该 Release 是否已含同名附件；有则不重复传（如需替换请先在网页删除该附件后重跑）
            $assets = (& curl.exe -s "$apiBase/releases/$relId/attach_files?access_token=$token") | ConvertFrom-Json
            $hasApk = $false
            if ($assets) {
                foreach ($a in @($assets)) { if ($a.name -eq $ApkName) { $hasApk = $true } }
            }
            if ($hasApk) {
                Warn "Gitee 附件 $ApkName 已存在 → 跳过上传。如需替换，请在 Gitee 网页删除该附件后重跑本步。"
            } else {
                Info "上传 Gitee 附件 $ApkName ..."
                $resp = & curl.exe -s -X POST -F "access_token=$token" -F "files[]=@$stageApk" "$apiBase/releases/$relId/attach_files"
                if ($resp -notmatch $ApkName -and $resp -match 'error|message') { Warn "Gitee 附件响应需人工核对：$resp" }
            }
            Ok "Gitee 附件（预期直链）：$giteeDl"
        }
    }

    # ══ 步骤 3：更新 dist 分支 remote-app-version.json ══
    if (-not $SkipDist) {
        $verObj = [ordered]@{
            version    = $Version
            apk        = $ApkName
            notes      = $Notes
            gitee_url  = $giteeDl
            github_url = $githubDl
        }
        $json = ($verObj | ConvertTo-Json -Depth 3)
        if ($DryRun) {
            Warn "[dry-run] 将写入 dist/remote-app-version.json："
            Write-Host $json
        } else {
            $Tmp = Join-Path $env:TEMP "va-dist-appver"
            if (Test-Path $Tmp) { Remove-Item $Tmp -Recurse -Force }
            $ErrorActionPreference = "Continue"
            git clone --depth 1 --branch dist $giteeUrl $Tmp 2>&1 | ForEach-Object { "$_" }
            $cloneOk = ($LASTEXITCODE -eq 0)
            $ErrorActionPreference = "Stop"
            if (-not $cloneOk) { throw "克隆 Gitee dist 分支失败" }
            try {
                # JSON 用 UTF-8 无 BOM 写（客户端 Dart jsonDecode 不吃 BOM）
                [System.IO.File]::WriteAllText((Join-Path $Tmp 'remote-app-version.json'), $json, (New-Object System.Text.UTF8Encoding($false)))
                Push-Location $Tmp
                git add remote-app-version.json
                git commit -m "dist: 遥控 App 版本清单 → $Version" --quiet 2>&1 | ForEach-Object { "$_" }
                if ($LASTEXITCODE -ne 0) { throw "无改动或提交失败（version.json 可能与线上一致）" }
                $ErrorActionPreference = "Continue"
                git push origin HEAD:refs/heads/dist 2>&1 | ForEach-Object { "$_" }
                $pushOk = ($LASTEXITCODE -eq 0)
                Pop-Location
                $ErrorActionPreference = "Stop"
                if (-not $pushOk) { throw "推送 dist 分支失败" }
            } finally {
                Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
            }
            Ok "dist/remote-app-version.json 已更新 → $Version"
        }
    }

    Ok "════════ 发布流程结束 ════════"
    if ($DryRun) { Warn "本次为 DRY-RUN，未对任何远端做出改动。去掉 -DryRun 正式执行。" }
    else {
        Write-Host ""
        Info "客户端将读取：https://gitee.com/$($gt.Owner)/$($gt.Repo)/raw/dist/remote-app-version.json（Gitee raw CDN 有缓存，线上生效可能延迟几分钟）"
        Info "请在真机点『设置-检查更新』验证：能发现 $Version 并可下载安装。"
    }
}
finally {
    Pop-Location
}
