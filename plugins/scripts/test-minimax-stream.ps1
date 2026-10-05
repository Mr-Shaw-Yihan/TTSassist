# MiniMax 流式 TTS（WebSocket T2A v2）实网冒烟脚本
#
# 不依赖宿主程序，独立验证「Key → WS 握手 → task 流 → 分块 PCM」全链路，
# 输出与流式插件对比直接相关的指标（首块延迟 / 块数 / 时长）。
#
# 用法：
#   1. Set env var:
#        $env:MINIMAX_STREAM_API_KEY = "your key"        # 必填（国内/国际通用同格式）
#        $env:MINIMAX_STREAM_REGION  = "domestic"        # 可选，默认 domestic；global=国际版
#   2. Run: pwsh -ExecutionPolicy Bypass -File test-minimax-stream.ps1
#   3. 产物：当前目录 minimax-stream-test.wav（可直接播放验证音质）

$ErrorActionPreference = "Stop"

$ApiKey  = $env:MINIMAX_STREAM_API_KEY
$Region  = if ($env:MINIMAX_STREAM_REGION) { $env:MINIMAX_STREAM_REGION } else { "domestic" }
$BaseUrl = if ($Region -eq "global") { "wss://api.minimax.io" } else { "wss://api.minimaxi.com" }
$Url     = "$BaseUrl/ws/v1/t2a_v2"

if (-not $ApiKey) {
    Write-Host "[SKIP] MINIMAX_STREAM_API_KEY not set:" -ForegroundColor Yellow
    Write-Host '  $env:MINIMAX_STREAM_API_KEY = "your key"'
    exit 1
}

$Model     = "speech-2.8-hd"
$VoiceId   = "female-tianmei"
$SampleRate = 32000
$TestText  = "你好，这是一段流式合成的测试。今天天气不错，适合出门散步。Streaming test second sentence."

# ── 连接 ──────────────────────────────────────────────────
Write-Host "Connecting: $Url" -ForegroundColor Cyan
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader("Authorization", "Bearer $ApiKey")
$ct = [System.Threading.CancellationToken]::None
$swTotal = [System.Diagnostics.Stopwatch]::StartNew()
$ws.ConnectAsync([Uri]$Url, $ct).GetAwaiter().GetResult()
Write-Host "[OK] connected (state=$($ws.State))" -ForegroundColor Green

function Receive-Message {
    $ms = New-Object System.IO.MemoryStream
    $buf = New-Object byte[] 65536
    while ($true) {
        $seg = [ArraySegment[byte]]::new($buf)
        $r = $ws.ReceiveAsync($seg, $ct).GetAwaiter().GetResult()
        if ($r.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
            return $null
        }
        $ms.Write($buf, 0, $r.Count)
        if ($r.EndOfMessage) { break }
    }
    return [System.Text.Encoding]::UTF8.GetString($ms.ToArray())
}

function Send-Json($obj) {
    $json = $obj | ConvertTo-Json -Depth 6 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $ct).GetAwaiter().GetResult()
}

# ── 握手：connected_success → task_start → task_started ────
$first = Receive-Message
if (-not $first) { throw "connection closed before connected_success" }
Write-Host "[RECV] $first"
$firstObj = $first | ConvertFrom-Json
if ($firstObj.base_resp.status_code -ne 0) { throw "handshake rejected: $($first | Out-String)" }

Send-Json @{
    event         = "task_start"
    model         = $Model
    voice_setting = @{ voice_id = $VoiceId; speed = 1.0; vol = 1.0; pitch = 0 }
    audio_setting = @{ sample_rate = $SampleRate; format = "pcm"; channel = 1 }
}

$started = Receive-Message
if (-not $started) { throw "connection closed waiting task_started" }
Write-Host "[RECV] $started"
$startedObj = $started | ConvertFrom-Json
if ($startedObj.event -ne "task_started") { throw "expected task_started: $($started | Out-String)" }

# ── 下发文本 + 收流 ────────────────────────────────────────
Send-Json @{ event = "task_continue"; text = $TestText }
Send-Json @{ event = "task_finish" }

$pcm = New-Object System.IO.MemoryStream
$firstChunkMs = $null
$chunks = 0
$done = $false
while (-not $done) {
    $swChunk = [System.Diagnostics.Stopwatch]::StartNew()
    $msg = Receive-Message
    if (-not $msg) { break }   # 服务端收尾断开
    if ($msg.Length -gt 220) { $msg = $msg.Substring(0, 220) + " ...(truncated)" }
    $obj = $msg | ConvertFrom-Json
    if ($obj.base_resp -and $obj.base_resp.status_code -ne 0) {
        throw "stream error $($obj.base_resp.status_code): $($obj.base_resp.status_msg)"
    }
    switch ($obj.event) {
        "task_continued" {
            if ($obj.data -and $obj.data.audio) {
                $hex = $obj.data.audio
                $bytes = New-Object byte[] ($hex.Length / 2)
                for ($i = 0; $i -lt $bytes.Length; $i++) {
                    $bytes[$i] = [Convert]::ToByte($hex.Substring($i * 2, 2), 16)
                }
                $pcm.Write($bytes, 0, $bytes.Length)
                $chunks++
                if ($null -eq $firstChunkMs) {
                    $firstChunkMs = $swTotal.ElapsedMilliseconds
                    Write-Host ("[FIRST AUDIO] {0} ms" -f $firstChunkMs) -ForegroundColor Green
                }
            }
            if ($obj.is_final) { $done = $true }
        }
        "task_finished" { $done = $true }
        "task_failed"   { throw "task_failed: $($msg | Out-String)" }
        default         { Write-Host "[RECV] $msg" }
    }
}
$swTotal.Stop()
$ws.Dispose()

if ($chunks -eq 0) { throw "no audio chunks received" }

# ── 封 WAV 落盘 ───────────────────────────────────────────
$pcmLen = [int]$pcm.Length
$wavPath = Join-Path (Get-Location) "minimax-stream-test.wav"
$fs = [System.IO.File]::Create($wavPath)
$bw = New-Object System.IO.BinaryWriter($fs)
$byteRate = $SampleRate * 2
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("RIFF"))
$bw.Write([uint32](36 + $pcmLen))
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("WAVEfmt "))
$bw.Write([uint32]16); $bw.Write([uint16]1); $bw.Write([uint16]1)
$bw.Write([uint32]$SampleRate); $bw.Write([uint32]$byteRate)
$bw.Write([uint16]2); $bw.Write([uint16]16)
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("data"))
$bw.Write([uint32]$pcmLen)
$pcm.Position = 0; $pcm.CopyTo($bw.BaseStream)
$bw.Close()

$audioMs = [int]($pcmLen / ($SampleRate * 2) * 1000)
Write-Host ""
Write-Host "========== RESULT ==========" -ForegroundColor Cyan
Write-Host ("chunks          : {0}" -f $chunks)
Write-Host ("first audio TTFB: {0} ms" -f $firstChunkMs)
Write-Host ("total elapsed   : {0} ms" -f $swTotal.ElapsedMilliseconds)
Write-Host ("audio duration  : {0} ms" -f $audioMs)
Write-Host ("wav file        : {0}" -f $wavPath)
Write-Host "[DONE] protocol OK — 插件侧同一协议实现，可放心填 Key 使用" -ForegroundColor Green
