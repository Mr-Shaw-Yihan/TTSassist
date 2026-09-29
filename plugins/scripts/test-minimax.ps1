# MiniMax TTS API Test Script
#
# Usage:
#   1. Set env vars:
#      $env:MINIMAX_API_KEY = "your domestic key"
#      $env:MINIMAX_GLOBAL_API_KEY = "your international key"
#   2. Run: powershell -ExecutionPolicy Bypass -File test-minimax.ps1

$ErrorActionPreference = "Continue"

$testText = "Hello, this is a test. Ni hao, zhe shi yi duan ce shi."
$endpoints = @()

if ($env:MINIMAX_API_KEY) {
    $endpoints += @{ Name = "Domestic"; Url = "https://api.minimaxi.com/v1/t2a_v2"; Key = $env:MINIMAX_API_KEY }
} else {
    Write-Host "[SKIP] Domestic: MINIMAX_API_KEY not set" -ForegroundColor Yellow
}

if ($env:MINIMAX_GLOBAL_API_KEY) {
    $endpoints += @{ Name = "Global"; Url = "https://api.minimax.io/v1/t2a_v2"; Key = $env:MINIMAX_GLOBAL_API_KEY }
} else {
    Write-Host "[SKIP] Global: MINIMAX_GLOBAL_API_KEY not set" -ForegroundColor Yellow
}

if ($endpoints.Count -eq 0) {
    Write-Host ""
    Write-Host "No API key configured. Set at least one:" -ForegroundColor Red
    Write-Host '  $env:MINIMAX_API_KEY = "your domestic key"'
    Write-Host '  $env:MINIMAX_GLOBAL_API_KEY = "your international key"'
    exit 1
}

foreach ($ep in $endpoints) {
    Write-Host ""
    Write-Host "========== Testing $($ep.Name): $($ep.Url) ==========" -ForegroundColor Cyan

    $body = @{
        model = "speech-2.8-hd"
        text  = $testText
        stream = $false
        voice_setting = @{
            voice_id = "female-tianmei"
            speed    = 1.0
            vol      = 1.0
            pitch    = 0
        }
        audio_setting = @{
            sample_rate = 32000
            bitrate     = 128000
            format      = "mp3"
            channel     = 1
        }
    } | ConvertTo-Json -Depth 5

    try {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()

        $resp = Invoke-RestMethod -Uri $ep.Url `
            -Method Post `
            -ContentType "application/json" `
            -Headers @{ Authorization = "Bearer $($ep.Key)" } `
            -Body ([System.Text.Encoding]::UTF8.GetBytes($body))

        $sw.Stop()

        if ($resp.base_resp.status_code -eq 0) {
            $hex = $resp.data.audio
            $audioBytes = [byte[]]::new($hex.Length / 2)
            for ($i = 0; $i -lt $hex.Length; $i += 2) {
                $audioBytes[$i / 2] = [Convert]::ToByte($hex.Substring($i, 2), 16)
            }

            $outFile = Join-Path $PSScriptRoot "test_$($ep.Name).mp3"
            [System.IO.File]::WriteAllBytes($outFile, $audioBytes)

            $sizeKB = [math]::Round($audioBytes.Length / 1024, 1)
            $durationMs = $resp.extra_info.audio_length
            $chars = $resp.extra_info.usage_characters

            Write-Host "  OK!" -ForegroundColor Green
            Write-Host "  Time: $($sw.ElapsedMilliseconds)ms"
            Write-Host "  Audio size: ${sizeKB}KB"
            Write-Host "  Duration: ${durationMs}ms"
            Write-Host "  Billed chars: $chars"
            Write-Host "  Saved: $outFile" -ForegroundColor Yellow
        } else {
            Write-Host "  API Error: $($resp.base_resp.status_code) - $($resp.base_resp.status_msg)" -ForegroundColor Red
        }
    } catch {
        Write-Host "  Request failed: $_" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "Done." -ForegroundColor Cyan
