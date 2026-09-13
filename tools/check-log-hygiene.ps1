# 日志泄漏守卫（T4.B）：§3.4-A「默认不含用户文本」的制度化。
# 扫 src-tauri/src/**/*.rs：任何 log_info! / log_warn! / log_error! 宏调用内
# （含跨行续体）出现已知「用户文本变量」标识作为插值参数 → exit 1。
#   - 用户文本标识清单：text / content / title / body / user_input / prompt
#   - 判定「插值参数」：格式串内联插值 {text}，或位置参数段中出现该标识（词边界）
#   - 误报豁免：宏调用块内任意位置写 // allowlog: <理由> 即放行该调用
# 目的：防止有人顺手 log_info!("合成失败: {text}") 把用户的话写进（可能进诊断包的）日志。

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

$files = Get-ChildItem -LiteralPath $srcDir -Recurse -File -Filter *.rs
foreach ($f in $files) {
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
    Write-Host "✗ 日志泄漏守卫：以下 log_*! 调用把用户文本变量写进了日志（清单: text/content/title/body/user_input/prompt）："
    $violations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  确认为误报时在该宏调用块内加行内豁免注释：// allowlog: <理由>"
    exit 1
}

Write-Host "✓ 日志泄漏检查通过（$($files.Count) 个 .rs，无用户文本标识入日志）"
