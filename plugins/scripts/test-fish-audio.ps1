# Fish Audio TTS API Test Script
#
# Usage:
#   1. Set env var:
#      $env:FISH_AUDIO_API_KEY = "sk-fish-..."          # 真实 Key
#      $env:FISH_SMOKE_PROXY   = "http://127.0.0.1:7897" # 可选，直连不通时设置
#   2. Run: pwsh -ExecutionPolicy Bypass -File test-fish-audio.ps1
#
# 覆盖：模型列表可达性 / 默认音色合成 / 指定音色合成 / 低延迟档 / 401 错误路径。
# 产物：%TEMP%\fish-audio-smoke\*.mp3（可直接播放试听）

$ErrorActionPreference = "Continue"

$Base   = "https://api.fish.audio"
$Key    = $env:FISH_AUDIO_API_KEY
$Proxy  = $env:FISH_SMOKE_PROXY
$OutDir = Join-Path $env:TEMP "fish-audio-smoke"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

if (-not $Key) {
    Write-Host "No API key configured. Set: `$env:FISH_AUDIO_API_KEY = 'sk-fish-...'" -ForegroundColor Red
    exit 1
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# curl.exe + UTF-8 body 文件（中文经控制台直传会被按 GBK 编码坑掉）
function Invoke-FishJson {
    param([string]$Method, [string]$Url, [string]$BodyJson, [string]$OutFile, [string]$KeyOverride)
    $bodyFile = Join-Path $OutDir "_body.json"
    [System.IO.File]::WriteAllText($bodyFile, $BodyJson, $Utf8NoBom)
    $args = @("-sS", "-m", "90", "-o", $OutFile, "-w", "%{http_code}", "-X", $Method.ToUpperInvariant(), $Url,
              "-H", "Authorization: Bearer $(if ($KeyOverride) { $KeyOverride } else { $Key })",
              "-H", "model: s2.1-pro-free")
    if ($Proxy) { $args += @("-x", $Proxy) }
    if ($BodyJson) { $args += @("-H", "Content-Type: application/json", "--data-binary", "@$bodyFile") }
    $code = & curl.exe @args
    return @{ Code = $code; File = $OutFile }
}

function Test-Tts {
    param([string]$Name, [string]$BodyJson, [string]$OutFile, [string]$KeyOverride)
    Write-Host ""
    Write-Host "---------- $Name ----------" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $r = Invoke-FishJson -Method Post -Url "$Base/v1/tts" -BodyJson $BodyJson -OutFile $OutFile -KeyOverride $KeyOverride
    $sw.Stop()
    $size = 0
    if (Test-Path $r.File) { $size = (Get-Item $r.File).Length }
    Write-Host ("HTTP {0}, {1} bytes, {2:N2}s" -f $r.Code, $size, $sw.Elapsed.TotalSeconds)
    if ($r.Code -eq "200") {
        Write-Host "PASS → $($r.File)" -ForegroundColor Green
        return $true
    } else {
        if ($size -gt 0 -and $size -lt 2000) { Get-Content $r.File -Raw | Write-Host -ForegroundColor Yellow }
        Write-Host "FAIL" -ForegroundColor Red
        return $false
    }
}

# ── 0. 可达性 + Key 有效性：模型列表 ──
Write-Host "========== 0. List models (reachability) ==========" -ForegroundColor Cyan
$r = Invoke-FishJson -Method Get -Url "$Base/model?page_size=3&page_number=1&sort_by=task_count" -BodyJson "" -OutFile (Join-Path $OutDir "models.json")
if ($r.Code -eq "200") {
    Write-Host "PASS：API 可达，Key 有效" -ForegroundColor Green
} elseif ($r.Code -eq "401") {
    Write-Host "FAIL：Key 无效（401）" -ForegroundColor Red; exit 1
} else {
    Write-Host "WARN：HTTP $($r.Code)（若超时，设置 `$env:FISH_SMOKE_PROXY 后重试）" -ForegroundColor Yellow
}

# ── 1. 默认音色（无 reference_id）──
$null = Test-Tts -Name "1. Default voice (s2.1-pro-free)" `
    -BodyJson '{"text":"你好，这是电子声带通过 Fish Audio 合成的语音。","format":"mp3","latency":"normal"}' `
    -OutFile (Join-Path $OutDir "default.mp3")

# ── 2. 指定音色（央视配音，插件内置音色表之一）──
$null = Test-Tts -Name "2. Reference voice (央视配音)" `
    -BodyJson '{"text":"今天天气不错，适合出门散步。","reference_id":"59cb5986671546eaa6ca8ae6f29f6d22","format":"mp3","latency":"normal"}' `
    -OutFile (Join-Path $OutDir "voice.mp3")

# ── 3. 低延迟档 ──
$null = Test-Tts -Name "3. Latency=low" `
    -BodyJson '{"text":"短句测试。","format":"mp3","latency":"low"}' `
    -OutFile (Join-Path $OutDir "low.mp3")

# ── 4. 错误路径：坏 Key 应返回 401 ──
Write-Host ""
Write-Host "---------- 4. Invalid key (expect 401) ----------" -ForegroundColor Cyan
$r = Invoke-FishJson -Method Post -Url "$Base/v1/tts" -BodyJson '{"text":"x"}' -OutFile (Join-Path $OutDir "err401.json") -KeyOverride "sk-fish-invalid000"
$body = Get-Content (Join-Path $OutDir "err401.json") -Raw
if ($r.Code -eq "401" -and $body -match "Invalid") {
    Write-Host "PASS：HTTP 401 Invalid Token（错误路径符合预期）" -ForegroundColor Green
} else {
    Write-Host "FAIL：期望 401，实际 HTTP $($r.Code)：$body" -ForegroundColor Red
}

Write-Host ""
Write-Host "试听产物目录：$OutDir"
