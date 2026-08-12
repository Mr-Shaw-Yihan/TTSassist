# 单独把 plugins/remote-config.json 推送到 Gitee dist 分支。
# 用途：只改邀请码等远程配置时，不必重跑 publish.ps1（不用重新打包插件）。
# 原理：在现有 dist 分支内容上追加/覆盖该文件后 fast-forward 推送，不动索引和 zip。
# 用法：powershell -ExecutionPolicy Bypass -File plugins\sync-remote-config.ps1

param()

$ErrorActionPreference = "Stop"

$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $PluginsDir
$GiteeRemote = "gitee"
$ConfigFile = Join-Path $PluginsDir "remote-config.json"

if (-not (Test-Path $ConfigFile)) { throw "找不到 $ConfigFile" }

# 校验 JSON 合法性，避免推坏配置
try { Get-Content $ConfigFile -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null }
catch { throw "remote-config.json 不是合法 JSON：$_" }

Push-Location $RepoRoot
$OriginBranch = (git rev-parse --abbrev-ref HEAD 2>&1 | Out-String).Trim()
try {
    git fetch $GiteeRemote dist 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "拉取 Gitee dist 分支失败（检查 gitee remote 是否配置）" }

    # 基于远端 dist 当前内容检出（detach），保留索引和 zip
    git checkout --detach FETCH_HEAD --quiet 2>&1 | Out-Null
    Copy-Item $ConfigFile -Destination . -Force
    git add remote-config.json
    git commit -m "dist: 更新远程配置（remote-config.json）" --quiet
    $ErrorActionPreference = "Continue"
    git push $GiteeRemote HEAD:refs/heads/dist 2>&1 | ForEach-Object { "$_" }
    $PushOk = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = "Stop"
    if (-not $PushOk) { throw "推送 dist 分支到 Gitee 失败" }
} finally {
    git checkout $OriginBranch --quiet 2>&1 | Out-Null
    Pop-Location
}

Write-Host "远程配置已同步 ✅  https://gitee.com/yihwan/TTSassist/raw/dist/remote-config.json" -ForegroundColor Green
Write-Host "提示：Gitee raw 有 CDN 缓存，客户端生效可能延迟几分钟；客户端自身缓存 24 小时。"
