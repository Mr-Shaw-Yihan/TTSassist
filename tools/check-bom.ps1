# BOM 守卫（T4.A）：把文档里的坑变成机器拦的坑。
# 规则（两个方向都要拦）：
#   - *.json：禁 BOM —— serde_json 不认 BOM，打了会导致解析失败（v1.8.2 事故根因）
#   - *.ps1：含非 ASCII（中文等）内容的必须有 BOM —— PowerShell 5.1 无 BOM 按 GBK 解码，
#             中文注释/字符串直接报错；纯 ASCII 脚本可不带（约定原文见 commit 1540d1a）
# 违规则列出文件名并 exit 1；扫描范围：仓库内全部 *.json / *.ps1
# （排除构建产物目录与 remote-app/ —— Flutter 端不在本仓库脚本约定内，见任务书 §二.5）。

$ErrorActionPreference = 'Stop'

# 仓库根 = tools/ 的上一级
$repoRoot = Split-Path -Parent $PSScriptRoot

$excludePattern = '\\(node_modules|target|dist|\.git|coverage|\.zcode|remote-app)\\'

$files = Get-ChildItem -LiteralPath $repoRoot -Recurse -File |
    Where-Object {
        ($_.Extension -eq '.json' -or $_.Extension -eq '.ps1') -and
        $_.FullName -notmatch $excludePattern
    }

if (-not $files) {
    Write-Error '未找到任何 .json / .ps1 文件，检查扫描范围'
}

$badNoBom  = @()   # .ps1 含非 ASCII 但缺 BOM（应带）
$asciiNoBomCount = 0
$badWithBom = @()  # .json 带 BOM（禁带）

foreach ($f in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    $head0 = if ($bytes.Length -ge 3) { $bytes[0] } else { 0 }
    $head1 = if ($bytes.Length -ge 3) { $bytes[1] } else { 0 }
    $head2 = if ($bytes.Length -ge 3) { $bytes[2] } else { 0 }
    $hasBom = ($bytes.Length -ge 3 -and $head0 -eq 0xEF -and $head1 -eq 0xBB -and $head2 -eq 0xBF)
    if ($f.Extension -eq '.json' -and $hasBom) { $badWithBom += $f }
    if ($f.Extension -eq '.ps1') {
        if ($hasBom) { continue }
        # 无 BOM 时再看内容：含 ≥0x80 字节（非 ASCII）才要求 BOM
        $nonAscii = $false
        foreach ($b in $bytes) { if ($b -ge 0x80) { $nonAscii = $true; break } }
        if ($nonAscii) { $badNoBom += $f } else { $asciiNoBomCount++ }
    }
}

$fail = $false
if ($badWithBom.Count -gt 0) {
    $fail = $true
    Write-Host "✗ 以下 .json 文件带 BOM（serde_json 会挂，必须无 BOM UTF-8）："
    $badWithBom | ForEach-Object { Write-Host ("    " + $_.FullName.Substring($repoRoot.Length + 1)) }
}
if ($badNoBom.Count -gt 0) {
    $fail = $true
    Write-Host "✗ 以下 .ps1 文件含非 ASCII 内容但缺 BOM（PowerShell 5.1 中文会乱码，必须 UTF-8 BOM）："
    $badNoBom | ForEach-Object { Write-Host ("    " + $_.FullName.Substring($repoRoot.Length + 1)) }
}

if ($fail) { exit 1 }

$ps1Count = ($files | Where-Object { $_.Extension -eq '.ps1' }).Count
$jsonCount = ($files | Where-Object { $_.Extension -eq '.json' }).Count
Write-Host "✓ BOM 检查通过：$jsonCount 个 .json 无 BOM；$ps1Count 个 .ps1（含 $asciiNoBomCount 个纯 ASCII 免检）规则合规"
