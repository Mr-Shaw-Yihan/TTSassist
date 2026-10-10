# 基座护栏（AGENTS.md「基座合同」的机械化）：本体（src-tauri/src、src）不得
# 出现具体引擎的身份字面量。引擎差异只能以能力位/自述数据存在（manifest、
# voices、ABI 符号），写成 `if engine == "xxx"` 就是把某个引擎的知识装回基座。
#
# 用法：
#   pwsh tools/check-base-hygiene.ps1                  # 检查（CI 用）
#   pwsh tools/check-base-hygiene.ps1 -UpdateBaseline   # 重写基线（拆完残留后收紧）
#
# 基线文件 tools/base-hygiene-baseline.txt，每行 `相对路径|模式|允许次数`：
#   - 出现基线里没有的 (路径, 模式)        → 红
#   - 某条计数比基线大                    → 红（登记过的残留只许变少不许变多）
#   - 计数变小/条目消失                    → 绿，并打印“可收紧”提示
# 行内豁免：`// baseok: <理由>` 所在行不计（与 // allowlog: 同口径）。
# 注：plugins/ 目录不扫——插件里写自家引擎协议是它本该干的活。

param(
    [switch]$UpdateBaseline
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $PSScriptRoot 'base-hygiene-baseline.txt'

# 引擎身份字面量（大小写不敏感；xiaomimimo 被 mimo 覆盖，voiceclone 被 mimo 所在的
# 插件独有，仍单独列出以便单独计数）。**接入新引擎时先往这里加一条**，
# 于是本体里所有“写死了这个引擎”的地方当场变红——这就是本护栏的全部目的。
$patterns = @('mimo', 'minimax', 'hojo', 'genie', 'edge-tts', 'moss', 'voiceclone')

# 扫描根一律用正斜杠：CI 跑在 ubuntu-latest 上，`src-tauri\src` 在 Linux 下是
# 一个不存在的路径（Windows 接受正斜杠，两边同一写法才能扫到同一批文件）
$scanRoots = @('src-tauri/src', 'src')
$fileGlobs = @('*.rs', '*.ts', '*.tsx')

$hits = @{}
$fileCount = 0
foreach ($root in $scanRoots) {
    $abs = Join-Path $repoRoot $root
    if (-not (Test-Path -LiteralPath $abs)) {
        Write-Error "找不到扫描目录：$abs"
    }
    foreach ($glob in $fileGlobs) {
        # 用 -Filter 而不是 -Include：后者与 -LiteralPath 组合在 PS 5.1 下不生效（踩过）
        $files = Get-ChildItem -LiteralPath $abs -Recurse -File -Filter $glob
        foreach ($f in $files) {
            $fileCount++
            $rel = $f.FullName.Substring($repoRoot.Length + 1) -replace '\\', '/'
            $text = [System.IO.File]::ReadAllText($f.FullName)
            foreach ($line in ($text -split "`r?`n")) {
                if ($line -match '//\s*baseok:') { continue }
                foreach ($p in $patterns) {
                    $c = ([regex]::Matches($line, $p, 'IgnoreCase')).Count
                    if ($c -gt 0) {
                        $key = "$rel|$p"
                        if ($hits.ContainsKey($key)) { $hits[$key] += $c } else { $hits[$key] = $c }
                    }
                }
            }
        }
    }
}

$current = @{}
foreach ($k in $hits.Keys) { $current[$k] = $hits[$k] }

# 生成/更新基线（按字典序写，无 BOM；行尾固定 CRLF 避免 diff 噪声）
if ($UpdateBaseline) {
    $out = @($current.Keys | Sort-Object | ForEach-Object { "{0}|{1}" -f $_, $current[$_] })
    [System.IO.File]::WriteAllText(
        $baselinePath,
        (($out -join "`r`n") + "`r`n"),
        (New-Object System.Text.UTF8Encoding($false))
    )
    Write-Host ("✓ 已写基线：{0} 条（扫描 {1} 个文件）→ tools/base-hygiene-baseline.txt" -f $out.Count, $fileCount)
    exit 0
}

if (-not (Test-Path -LiteralPath $baselinePath)) {
    Write-Host "✗ 找不到基线文件 tools/base-hygiene-baseline.txt"
    Write-Host "  本地先跑一次：pwsh tools/check-base-hygiene.ps1 -UpdateBaseline（人工核对后提交）"
    exit 1
}

$baseline = @{}
foreach ($l in [System.IO.File]::ReadAllLines($baselinePath)) {
    $t = $l.Trim()
    if (-not $t -or $t.StartsWith('#')) { continue }
    $parts = $t -split '\|'
    if ($parts.Count -lt 3) { continue }
    $k = $parts[0] + '|' + $parts[1]
    # 同一路径+模式写了多行时取累加（容忍手工编辑时的无意重复）
    $n = 0
    [void][int]::TryParse($parts[2], [ref]$n)
    if ($baseline.ContainsKey($k)) { $baseline[$k] += $n } else { $baseline[$k] = $n }
}

$violations = @()
foreach ($k in ($current.Keys | Sort-Object)) {
    if (-not $baseline.ContainsKey($k)) {
        $violations += ("{0}  → {1} 处（基线未登记）" -f $k, $current[$k])
    } elseif ($current[$k] -gt $baseline[$k]) {
        $violations += ("{0}  → 当前 {1} 处 > 基线 {2} 处（登记过的残留只许变少）" -f $k, $current[$k], $baseline[$k])
    }
}

if ($violations.Count -gt 0) {
    Write-Host "✗ 基座护栏：本体里出现了引擎身份（见 AGENTS.md 基座合同）："
    $violations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  正确做法：把这段判断搬进插件（manifest / voices.label / ABI 自述）；"
    Write-Host "  确属本次无法消除的历史遗留，一并用 -UpdateBaseline 重写基线，并在 commit message 里说明原因。"
    exit 1
}

# 通过：提醒可以收紧的条目（不阻断，避免“改完还得跑一次刷基线”的死循环）
$loosened = @()
foreach ($k in ($baseline.Keys | Sort-Object)) {
    if (-not $current.ContainsKey($k)) {
        $loosened += "$k（已消失，基线可从 {0} 降为 0）" -f $baseline[$k]
    } elseif ($current[$k] -lt $baseline[$k]) {
        $loosened += "{0}（{1} → {2}）" -f $k, $baseline[$k], $current[$k]
    }
}
if ($loosened.Count -gt 0) {
    Write-Host "· 残留已减少，建议收紧基线：pwsh tools/check-base-hygiene.ps1 -UpdateBaseline"
    $loosened | ForEach-Object { Write-Host "    $_" }
}
Write-Host ("✓ 基座检查通过（扫描 {0} 个文件，命中 {1} 条均在基线内）" -f $fileCount, $current.Count)
