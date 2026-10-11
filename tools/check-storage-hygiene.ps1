# 存储护栏（AGENTS.md「存储契约」的机械化）：数据路径只能来自 `storage::paths`。
#
# 用法：
#   pwsh tools/check-storage-hygiene.ps1                  # 检查（CI 用）
#   pwsh tools/check-storage-hygiene.ps1 -UpdateBaseline   # 重写基线（清掉存量后收紧）
#
# 基线文件 tools/storage-hygiene-baseline.txt，每行 `相对路径|模式|允许次数`：
#   - 出现基线里没有的 (路径, 模式)  → 红
#   - 某条计数比基线大              → 红（登记过的只许变少不许变多）
#   - 计数变小 / 条目消失            → 红（棘轮必须当场收紧，理由同基座护栏）
# 行内豁免：`// storageok: <理由>` 写在命中行的本行或紧邻上一行。
#
# 只扫 src-tauri/src：路径拼接只发生在 Rust 侧，前端不碰文件系统。
# 不扫 src-tauri/src/storage/paths.rs：它**就是**路径的唯一主人，这些字面量本来
# 只该存在那一份；扫它只会刷出一堆无意义基线，反而稀释了「别的文件出现即红」的信号
#（与基座护栏不扫 plugins/ 同一思路——护栏管的是「不该知道这件事的地方知道了」）。

param(
    [switch]$UpdateBaseline
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $PSScriptRoot 'storage-hygiene-baseline.txt'

# 违规形态（大小写不敏感）：
# - `\.app_data_dir\(\)`：必须带前导点，才是真的在调 Tauri 的取目录方法；
#   注释与文档里写「app_data_dir」不会被误伤（实测 11 处提及里只有 lib.rs 一处是调用）
# - 三个内容目录的字面量拼接：收口后（Task 3c）业务代码里应当为零，剩下的只在测试脚手架
# - `LocalDataDir` / `app_cache_dir` 分两条写：模式里带竖线会破坏基线的三列格式，
#   下面的守卫会当场拒绝（所以宁可拆成两条也不能合成一条正则）
$patterns = @(
    '\.app_data_dir\(\)',
    '\.join\("audio"\)',
    '\.join\("voice_samples"\)',
    '\.join\("plugin-data"\)',
    '\.join\("subtitle_history\.json"\)',
    'LocalDataDir',
    'app_cache_dir'
)

foreach ($p in $patterns) {
    if ($p.Contains('|')) { Write-Error "护栏模式不得包含竖线（会破坏基线格式）：$p" }
}

# 正斜杠扫描根：CI 跑在 ubuntu-latest，反斜杠路径在 Linux 下等于没扫
$scanRoot = Join-Path $repoRoot 'src-tauri/src'
if (-not (Test-Path -LiteralPath $scanRoot)) {
    Write-Error "找不到扫描目录：$scanRoot"
}
# 路径权威模块自身豁免（见文件头注）
$skipFiles = @('src-tauri/src/storage/paths.rs')

$hits = @{}
$fileCount = 0
foreach ($f in (Get-ChildItem -LiteralPath $scanRoot -Recurse -File -Filter '*.rs')) {
    $rel = $f.FullName.Substring($repoRoot.Length + 1) -replace '\\', '/'
    if ($skipFiles -contains $rel) { continue }
    $fileCount++
    $text = [System.IO.File]::ReadAllText($f.FullName)
    $prev = ''
    foreach ($line in ($text -split "`r?`n")) {
        $skip = ($line -match '//\s*storageok:') -or ($prev -match '//\s*storageok:')
        $prev = $line
        if ($skip) { continue }
        foreach ($p in $patterns) {
            $c = ([regex]::Matches($line, $p, 'IgnoreCase')).Count
            if ($c -gt 0) {
                $key = "$rel|$p"
                if ($hits.ContainsKey($key)) { $hits[$key] += $c } else { $hits[$key] = $c }
            }
        }
    }
}

$current = @{}
foreach ($k in $hits.Keys) { $current[$k] = $hits[$k] }

if ($UpdateBaseline) {
    $keys = @($current.Keys)
    [Array]::Sort([string[]]$keys, [StringComparer]::Ordinal)
    $out = @($keys | ForEach-Object { "{0}|{1}" -f $_, $current[$_] })
    [System.IO.File]::WriteAllText(
        $baselinePath,
        (($out -join "`r`n") + "`r`n"),
        (New-Object System.Text.UTF8Encoding($false))
    )
    Write-Host ("✓ 已写基线：{0} 条（扫描 {1} 个文件）→ tools/storage-hygiene-baseline.txt" -f $out.Count, $fileCount)
    exit 0
}

if (-not (Test-Path -LiteralPath $baselinePath)) {
    Write-Host "✗ 找不到基线文件 tools/storage-hygiene-baseline.txt"
    Write-Host "  本地先跑一次：pwsh tools/check-storage-hygiene.ps1 -UpdateBaseline（人工核对后提交）"
    exit 1
}

$baseline = @{}
$badLines = @()
$lineNo = 0
foreach ($l in [System.IO.File]::ReadAllLines($baselinePath)) {
    $lineNo++
    $t = $l.Trim()
    if (-not $t -or $t.StartsWith('#')) { continue }
    $parts = $t -split '\|'
    if ($parts.Count -ne 3) { $badLines += "第 $lineNo 行不是「路径|模式|次数」三列：$t"; continue }
    $k = $parts[0] + '|' + $parts[1]
    $n = 0
    if ((-not [int]::TryParse($parts[2], [ref]$n)) -or $n -lt 0) {
        $badLines += "第 $lineNo 行的次数不是非负整数：$t"; continue
    }
    if ($baseline.ContainsKey($k)) {
        $badLines += "第 $lineNo 行重复登记 $k（已有 $($baseline[$k])）：合并为一行，别靠累加放宽额度"; continue
    }
    $baseline[$k] = $n
}
if ($badLines.Count -gt 0) {
    Write-Host "✗ 基线文件本身有问题（tools/storage-hygiene-baseline.txt）："
    $badLines | ForEach-Object { Write-Host "    $_" }
    Write-Host "  坏行会被整条忽略或把允许次数放大，先修基线再谈代码"
    exit 1
}

$violations = @()
foreach ($k in ($current.Keys | Sort-Object)) {
    if (-not $baseline.ContainsKey($k)) {
        $violations += ("{0}  → {1} 处（基线未登记）" -f $k, $current[$k])
    } elseif ($current[$k] -gt $baseline[$k]) {
        $violations += ("{0}  → 当前 {1} 处 > 基线 {2} 处（登记过的只许变少）" -f $k, $current[$k], $baseline[$k])
    }
}

if ($violations.Count -gt 0) {
    Write-Host "✗ 存储护栏：数据路径没走 storage::paths（见 AGENTS.md 存储契约）："
    $violations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  正确做法：改用 storage::paths::StoragePaths 的 getter（audio_dir/samples_dir/…"
    Write-Host "  记录里的相对路径用 audio_rel/samples_rel）；确属测试脚手架或无法消除的历史"
    Write-Host "  遗留，一并用 -UpdateBaseline 重写基线，并在 commit message 里说明原因。"
    exit 1
}

$loosened = @()
foreach ($k in ($baseline.Keys | Sort-Object)) {
    if (-not $current.ContainsKey($k)) {
        $loosened += "$k（已消失，基线里这条该删）"
    } elseif ($current[$k] -lt $baseline[$k]) {
        $loosened += "{0}（{1} → {2}）" -f $k, $baseline[$k], $current[$k]
    }
}
if ($loosened.Count -gt 0) {
    Write-Host "✗ 基线过期：违规已减少，收紧后与代码改动放进同一个 commit："
    $loosened | ForEach-Object { Write-Host "    $_" }
    Write-Host "  跑：pwsh tools/check-storage-hygiene.ps1 -UpdateBaseline（只会把数字改小，第二次必绿）"
    exit 1
}
Write-Host ("✓ 存储检查通过（扫描 {0} 个文件，命中 {1} 条均与基线持平）" -f $fileCount, $current.Count)
# 显式收 0：否则本地用 `& script.ps1` 调用时 $LASTEXITCODE 会残留上一条命令的值
exit 0
