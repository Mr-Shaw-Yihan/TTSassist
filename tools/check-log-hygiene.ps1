# 日志泄漏守卫（T4.B + 第二轮 T-D 扩围）：§3.4-A「默认不含用户文本」的制度化。
# 扫描两个根：
#   1) 宿主 src-tauri/src/**/*.rs：log_info!/log_warn!/log_error! 内（含跨行续体）
#      出现「用户文本变量」插值 → 红（清单: text/content/title/body/user_input/prompt）
#   2) 插件 plugins/*/src/**/*.rs（跳过 target）：
#      规则 a：println!/eprintln! 调用必须带 // allowlog: 豁免注释（宿主不捕获插件
#              stdout，这些输出本来就不该存在，结构性日志需显式写明理由）
#      规则 b：Err(format!(...)) 格式串内插命中黑名单 → 红
#              （清单: text/body/resp/response/content/transcript/result/data/chunk/delta）
#   - 误报豁免：调用块内任意位置写 // allowlog: <理由> 即放行
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$srcDir = Join-Path $repoRoot 'src-tauri\src'

if (-not (Test-Path -LiteralPath $srcDir)) {
    Write-Error "找不到 src-tauri/src：$srcDir"
}

$markers = @('text', 'content', 'title', 'body', 'user_input', 'prompt')
$identPattern = '\b(text|content|title|body|user_input|prompt)\b'
# 格式串内的内联插值：{ ... text ... }（花括号内无嵌套花括号）
$inlinePattern = '\{[^{}]*\b(text|content|title|body|user_input|prompt)\b[^{}]*\}'
# 插件规则 b 的 Err(format!) 黑名单与内插模式
$pluginErrPattern = '\{[^{}]*\b(text|body|resp|response|content|transcript|result|data|chunk|delta)\b[^{}]*\}'

$violations = @()

# 从 startIdx（log_xxx! 起始）提取完整宏调用参数块：括号平衡 + 字符串字面量状态机
function Get-MacroBlock {
    param([string]$Text, [int]$OpenParenIdx)
    $depth = 0
    $inString = $false
    $i = $OpenParenIdx
    $len = $Text.Length
    while ($i -lt $len) {
        $c = $Text[$i]
        if ($inString) {
            if ($c -eq '\') { $i += 2; continue }   # 跳过转义对（\" 等）
            if ($c -eq '"') { $inString = $false }
        } else {
            if ($c -eq '"') { $inString = $true }
            elseif ($c -eq '(') { $depth++ }
            elseif ($c -eq ')') {
                $depth--
                if ($depth -eq 0) { return $Text.Substring($OpenParenIdx + 1, $i - $OpenParenIdx - 1) }
            }
        }
        $i++
    }
    return $Text.Substring([Math]::Min($OpenParenIdx + 1, $len))  # 未闭合（残缺文件）取到尾
}

# 计算字符串第 upTo 索引前的换行数 → 行号
function Get-LineOfIndex { param([string]$Text, [int]$Idx)
    ($Text.Substring(0, $Idx) -split "`n").Count
}

$hostFiles = Get-ChildItem -LiteralPath $srcDir -Recurse -File -Filter *.rs
foreach ($f in $hostFiles) {
    $content = [System.IO.File]::ReadAllText($f.FullName)
    $matches0 = [regex]::Matches($content, '\blog_(info|warn|error)!\s*\(')
    foreach ($m in $matches0) {
        $block = Get-MacroBlock -Text $content -OpenParenIdx ($m.Index + $m.Value.Length - 1)
        if ($block -match '//\s*allowlog:') { continue }   # 行内豁免

        $hit = $null
        if ($block -match $inlinePattern) {
            $hit = "内联插值 $($Matches[0])"
        } else {
            # 顶层逗号分割：第 0 段是格式串（内联插值已查过），其余段出现标识即违规
            $depth = 0; $inString = $false; $segs = New-Object System.Collections.Generic.List[string]
            $cur = New-Object System.Text.StringBuilder
            $chars = $block.ToCharArray()
            for ($i = 0; $i -lt $chars.Length; $i++) {
                $c = $chars[$i]
                if ($inString) {
                    [void]$cur.Append($c)
                    if ($c -eq '\') { $i++; if ($i -lt $chars.Length) { [void]$cur.Append($chars[$i]) }; continue }
                    if ($c -eq '"') { $inString = $false }
                    continue
                }
                switch ($c) {
                    '"' { $inString = $true; [void]$cur.Append($c); continue }
                    '(' { $depth++ }
                    ')' { $depth-- }
                }
                if ($c -eq ',' -and $depth -eq 0) { $segs.Add($cur.ToString()); [void]$cur.Clear(); continue }
                [void]$cur.Append($c)
            }
            $segs.Add($cur.ToString())
            if ($segs.Count -gt 1) {
                foreach ($seg in ($segs | Select-Object -Skip 1)) {
                    if ($seg -match $identPattern) {
                        $hit = "位置参数含标识 $($Matches[1])"
                        break
                    }
                }
            }
        }
        if ($hit) {
            $line = Get-LineOfIndex -Text $content -Idx $m.Index
            $violations += ("{0}:{1}  [{2}]" -f $f.FullName.Substring($repoRoot.Length + 1), $line, $hit)
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Host "✗ 日志泄漏守卫：以下调用把用户文本变量写进了日志/输出："
    $violations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  确认为误报时在该宏调用块内加行内豁免注释：// allowlog: <理由>"
    exit 1
}

# ── 插件目录（T-D 扩围）：规则 a + 规则 b ──────────────────
$pluginsDir = Join-Path $repoRoot 'plugins'
$pluginFiles = @()
if (Test-Path -LiteralPath $pluginsDir) {
    $pluginFiles = Get-ChildItem -LiteralPath $pluginsDir -Recurse -File -Filter *.rs |
        Where-Object { $_.FullName -notmatch '\\target\\' }
}

$pluginViolations = @()
foreach ($f in $pluginFiles) {
    $content = [System.IO.File]::ReadAllText($f.FullName)

    # 规则 a：println!/eprintln! 必须带 // allowlog: 豁免（含跨行续体）。
    # 检查范围 = 调用起始行整行（行尾豁免）+ 前一行（紧邻豁免）+ 宏块内容（块内豁免）
    foreach ($m in [regex]::Matches($content, '\b(println|eprintln)!\s*\(')) {
        $block = Get-MacroBlock -Text $content -OpenParenIdx ($m.Index + $m.Value.Length - 1)
        $lineStart = $content.LastIndexOf("`n", [Math]::Max(0, $m.Index - 1)) + 1
        $lineText = ($content.Substring($lineStart) -split "`n")[0]
        $prevText = ""
        if ($lineStart -gt 0) {
            $prevStart = $content.LastIndexOf("`n", [Math]::Max(0, $lineStart - 2)) + 1
            $prevText = ($content.Substring($prevStart) -split "`n")[0]
        }
        $checkScope = "$prevText`n$lineText`n$block"
        if ($checkScope -match '//\s*allowlog:') { continue }
        $line = Get-LineOfIndex -Text $content -Idx $m.Index
        $pluginViolations += ("{0}:{1}  [插件 println!/eprintln! 缺 // allowlog: 豁免]" -f
            $f.FullName.Substring($repoRoot.Length + 1), $line)
    }

    # 规则 b：Err(format!...) 格式串内插命中黑名单
    foreach ($m in [regex]::Matches($content, 'Err\(\s*format!\s*\(')) {
        $block = Get-MacroBlock -Text $content -OpenParenIdx ($m.Index + $m.Value.Length - 1)
        if ($block -match '//\s*allowlog:') { continue }
        # 只取格式串（第一个字符串字面量）里的内插
        if ($block -match '"((?:[^"\\]|\\.)*)"') {
            $fmt = $Matches[1]
            if ($fmt -match $pluginErrPattern) {
                $line = Get-LineOfIndex -Text $content -Idx $m.Index
                $pluginViolations += ("{0}:{1}  [Err(format!) 内插命中黑名单: {2}]" -f
                    $f.FullName.Substring($repoRoot.Length + 1), $line, $Matches[0])
            }
        }
    }
}

if ($pluginViolations.Count -gt 0) {
    Write-Host "✗ 日志泄漏守卫（插件目录）：以下调用违反插件输出/错误脱敏规则："
    $pluginViolations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  println!/eprintln! 属结构性日志时加 // allowlog: <理由>；Err 内插用户数据必须改为不打内容"
    exit 1
}

Write-Host ("✓ 日志泄漏检查通过（宿主 {0} 个 + 插件 {1} 个 .rs，无用户文本标识入日志）" -f
    $hostFiles.Count, $pluginFiles.Count)
