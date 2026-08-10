# 一次性脚本：构建 Gitee dist 分发分支（后续同步逻辑合入 publish.ps1）。
# 流程：拉线上索引 → 下载线上 zip（与 Release 逐字节一致）→ 注入 mirror_url/plugin_type
#       → 建 orphan dist 分支 → force push 到 Gitee。
# 用法：powershell -ExecutionPolicy Bypass -File .\build-gitee-dist.ps1

$ErrorActionPreference = "Stop"

$GiteeRemote = "gitee"
$GiteeBase = "https://gitee.com/yihwan/TTSassist"
$Work = Join-Path $env:TEMP "gitee-dist-build"
$RepoDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)  # VoiceAssist/
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# plugin_type 映射（新 publish.ps1 会把此字段写进索引；线上旧索引还没有，这里补齐）
$TypeOf = @{ "edge-tts" = "tts_engine"; "mimo-asr" = "asr_engine"; "genie-tts" = "tts_engine" }

# ── 1. 拉线上索引 ─────────────────────────────────────────
if (Test-Path $Work) { Remove-Item $Work -Recurse -Force }
New-Item -ItemType Directory -Path $Work | Out-Null

$IndexUrl = "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json"
Write-Host "拉取线上索引…" -ForegroundColor Cyan
Invoke-WebRequest -Uri $IndexUrl -OutFile (Join-Path $Work "plugins-index.json") -TimeoutSec 60
$Index = Get-Content (Join-Path $Work "plugins-index.json") -Raw | ConvertFrom-Json

# ── 2. 下载全部 zip 并校验 SHA-256（与线上 Release 逐字节一致） ──
foreach ($p in $Index.plugins) {
    $ZipName = Split-Path $p.download_url -Leaf
    Write-Host "下载 $ZipName …" -ForegroundColor Cyan
    Invoke-WebRequest -Uri $p.download_url -OutFile (Join-Path $Work $ZipName) -TimeoutSec 600
    $Sha = (Get-FileHash (Join-Path $Work $ZipName) -Algorithm SHA256).Hash.ToLower()
    if ($Sha -ne $p.checksum.ToLower()) {
        throw "$ZipName 校验和不匹配：本地 $Sha ≠ 索引 $($p.checksum)"
    }
    Write-Host "  ✓ SHA-256 一致" -ForegroundColor Green
}

# ── 3. 重建索引：补 plugin_type + 注入 mirror_url（指向 Gitee dist 分支 raw） ──
$Entries = @()
foreach ($p in $Index.plugins) {
    $ZipName = Split-Path $p.download_url -Leaf
    $Entries += [ordered]@{
        id           = $p.id
        name         = $p.name
        version      = $p.version
        download_url = $p.download_url
        mirror_url   = "$GiteeBase/raw/dist/$ZipName"
        checksum     = $p.checksum
        description  = $p.description
        plugin_type  = if ($p.plugin_type) { $p.plugin_type } else { $TypeOf[$p.id] }
    }
}
$NewIndex = [ordered]@{ plugins = @($Entries) }
[System.IO.File]::WriteAllText(
    (Join-Path $Work "plugins-index.json"),
    ($NewIndex | ConvertTo-Json -Depth 5),
    $Utf8NoBom
)
Write-Host "索引已注入 mirror_url / plugin_type" -ForegroundColor Green

# ── 4. 建 orphan dist 分支（无历史，体积最小）并推送 ──────
Push-Location $RepoDir
try {
    git checkout --orphan dist-tmp 2>&1 | Out-Null
    # 物理删掉工作区的 tracked 文件（ignored 目录 node_modules/target 不受影响，也不会被 add）
    git rm -rf --quiet . 2>&1 | Out-Null
    Get-ChildItem $Work -File | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $RepoDir $_.Name) -Force
    }
    git add -A
    git commit -m "dist: 插件分发镜像（索引 + zip，供国内 Gitee 通道下载）" --quiet
    git push $GiteeRemote dist-tmp:dist --force 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { throw "推送 dist 分支到 Gitee 失败" }
    Write-Host ""
    Write-Host "dist 分支已推送 ✅  索引地址: $GiteeBase/raw/dist/plugins-index.json" -ForegroundColor Green
} finally {
    # 无论成败都回到 main 并清理临时分支与工作区里的镜像文件
    git checkout main --quiet 2>&1 | Out-Null
    git branch -D dist-tmp 2>&1 | Out-Null
    git clean -fd --quiet 2>&1 | Out-Null
    Pop-Location
}
