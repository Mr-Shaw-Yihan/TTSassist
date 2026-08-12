# 单独把 plugins/remote-config.json 推送到 Gitee dist 分支。
# 用途：只改邀请码等远程配置时，不必重跑 publish.ps1（不用重新打包插件）。
# 原理：临时克隆 dist 分支 → 覆盖该文件 → 提交推送，完全不碰本地仓库工作区。
# 用法：powershell -ExecutionPolicy Bypass -File plugins\sync-remote-config.ps1

param()

$ErrorActionPreference = "Stop"

$PluginsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $PluginsDir
$ConfigFile = Join-Path $PluginsDir "remote-config.json"

if (-not (Test-Path $ConfigFile)) { throw "找不到 $ConfigFile" }

# 校验 JSON 合法性，避免推坏配置
try { Get-Content $ConfigFile -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null }
catch { throw "remote-config.json 不是合法 JSON：$_" }

Push-Location $RepoRoot
$GiteeUrl = (git remote get-url gitee 2>&1 | Out-String).Trim()
Pop-Location
if (-not $GiteeUrl) { throw "未配置 gitee remote" }

# 临时浅克隆 dist 分支（约 12MB），操作完即删
$Tmp = Join-Path $env:TEMP "va-dist-config-sync"
if (Test-Path $Tmp) { Remove-Item $Tmp -Recurse -Force }
$ErrorActionPreference = "Continue"   # git 的 stderr 信息在 Stop 策略下会中断脚本，降级只看退出码
git clone --depth 1 --branch dist $GiteeUrl $Tmp 2>&1 | ForEach-Object { "$_" }
if ($LASTEXITCODE -ne 0) { $ErrorActionPreference = "Stop"; throw "克隆 Gitee dist 分支失败" }

try {
    Copy-Item $ConfigFile (Join-Path $Tmp "remote-config.json") -Force
    Push-Location $Tmp
    git add remote-config.json
    git commit -m "dist: 更新远程配置（remote-config.json）" --quiet
    if ($LASTEXITCODE -ne 0) { throw "无改动或提交失败（配置内容可能与线上一致）" }
    git push origin HEAD:refs/heads/dist 2>&1 | ForEach-Object { "$_" }
    $PushOk = ($LASTEXITCODE -eq 0)
    Pop-Location
    $ErrorActionPreference = "Stop"
    if (-not $PushOk) { throw "推送 dist 分支到 Gitee 失败" }
} finally {
    Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "远程配置已同步 ✅  https://gitee.com/yihwan/TTSassist/raw/dist/remote-config.json" -ForegroundColor Green
Write-Host "提示：Gitee raw 有 CDN 缓存，客户端生效可能延迟几分钟；客户端自身缓存 24 小时。"
