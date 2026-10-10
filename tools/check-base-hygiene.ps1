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
#   - 计数变小 / 条目消失                  → 红（棘轮必须当场收紧）
# 为什么“变小也红”：若只拦“变大”，一段残留拆完后基线里留着一条白额度，
# 后来人在**同一文件重新写死同一引擎**照样绿（合同的核心不变量——只准单向
# 收紧——就不成立了）。收紧只多跑一次 -UpdateBaseline（它只会把数字改小，
# 第二次必绿），不存在“改完还得刷一次基线”的死循环。
# 行内豁免：`// baseok: <理由>` 写在命中行的**本行或紧邻上一行**即不计该行
#（与日志护栏的 // allowlog: 同口径——后者同样支持前一行豁免）。
# 注：plugins/ 目录不扫——插件里写自家引擎协议是它本该干的活。

param(
    [switch]$UpdateBaseline
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $PSScriptRoot 'base-hygiene-baseline.txt'

# 引擎身份字面量（大小写不敏感）。**接入新引擎时先往这里加一条**，
# 于是本体里所有“写死了这个引擎”的地方当场变红——这就是本护栏的全部目的。
# 别名/变体必须单独列，否则拼写差一点就整段绕过（审查 C-1 实测）：
# - `voice_clone` / `edge_voice`：引擎专有端点名与遗留字段名（连字符模式抓不到下划线）
# - 带引号的裸 `edge`：Task 7 迁移用的旧引擎标识（`Some("edge")`）；**不收裸词 edge**，
#   免得把 border/fallback/edge case 这类普通英文误伤。引号类要同时含双引号与
#   单引号（Rust/TS 双引为主，PS 单引号字面量里 `''` 才是一个 `'`，已核对有命中）
$patterns = @(
    'mimo', 'minimax', 'hojo', 'genie', 'edge-tts', 'moss', 'voiceclone',
    'voice_clone', 'edge_voice', '(?<=["''])edge(?=["''])'
)

# 基线以竖线分列，模式里出现竖线会把基线行解析成错位数据（当场拒绝而不是默默放跑）
foreach ($p in $patterns) {
    if ($p.Contains('|')) { Write-Error "护栏模式不得包含竖线（会破坏基线格式）：$p" }
}

# 扫描根一律用正斜杠：CI 跑在 ubuntu-latest 上，`src-tauri\src` 在 Linux 下是
# 一个不存在的路径（Windows 接受正斜杠，两边同一写法才能扫到同一批文件）
$scanRoots = @('src-tauri/src', 'src')
# .js/.jsx 也得扫：悬浮球动效引擎目录有 8 个 .js，不扫等于基座合同对 JS 不设防
#（.d.ts 已被 *.ts 抢着扫到，*.jsx 不在 *.tsx 的匹配范围内）
$fileGlobs = @('*.rs', '*.ts', '*.tsx', '*.js', '*.jsx')

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
            $prev = ''
            foreach ($line in ($text -split "`r?`n")) {
                # 豁免作用域：本行或紧邻上一行（allowlog 同样是前一行可豁免的口径）
                $skip = ($line -match '//\s*baseok:') -or ($prev -match '//\s*baseok:')
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
    }
}

$current = @{}
foreach ($k in $hits.Keys) { $current[$k] = $hits[$k] }

# 生成/更新基线。排序用 Ordinal（字节序）：文化敏感排序会因机器/ICU 版本不同
# 把 51 行整体重排，淹没真实改动的 review diff
if ($UpdateBaseline) {
    $keys = @($current.Keys)
    [Array]::Sort([string[]]$keys, [StringComparer]::Ordinal)
    $out = @($keys | ForEach-Object { "{0}|{1}" -f $_, $current[$_] })
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
$badLines = @()
$lineNo = 0
foreach ($l in [System.IO.File]::ReadAllLines($baselinePath)) {
    $lineNo++
    $t = $l.Trim()
    if (-not $t -or $t.StartsWith('#')) { continue }
    $parts = $t -split '\|'
    # 列数不对 = 基线被手工改坏了。静默 continue 会把它报成“代码新增了违规”，
    # 多一列则会把额度翻倍——两者都必须当场拦下（审查 I-4）
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
    Write-Host "✗ 基线文件本身有问题（tools/base-hygiene-baseline.txt）："
    $badLines | ForEach-Object { Write-Host "    $_" }
    Write-Host "  坏行会被整条忽略或把允许次数放大，先修基线再谈代码"
    exit 1
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

# 变小/消失同样阻断：棘轮必须当场收紧（否则白额度可以被子孙重新花掉，见头注）
$loosened = @()
foreach ($k in ($baseline.Keys | Sort-Object)) {
    if (-not $current.ContainsKey($k)) {
        $loosened += "$k（已消失，基线里这条该删）"
    } elseif ($current[$k] -lt $baseline[$k]) {
        $loosened += "{0}（{1} → {2}）" -f $k, $baseline[$k], $current[$k]
    }
}
if ($loosened.Count -gt 0) {
    Write-Host "✗ 基线过期：残留已减少，收紧后与代码改动放进同一个 commit："
    $loosened | ForEach-Object { Write-Host "    $_" }
    Write-Host "  跑：pwsh tools/check-base-hygiene.ps1 -UpdateBaseline（只会把数字改小，第二次必绿）"
    exit 1
}
Write-Host ("✓ 基座检查通过（扫描 {0} 个文件，命中 {1} 条均与基线持平）" -f $fileCount, $current.Count)
# 显式收 0：否则在本地用 `& script.ps1` 调用时 $LASTEXITCODE 会残留上一条命令的值，
# 看上去像护栏红了（CI 侧 pwsh 进程本身就是 0，仅影响本地取证与后续自动化）
exit 0
