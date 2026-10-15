# MOSS TTS（流式）实网冒烟：跑 moss-tts crate 内的 #[ignore] 探针
#
# 用法：
#   pwsh -ExecutionPolicy Bypass -File .\test-moss-stream.ps1            # release 跑（交付口径）
#   pwsh -ExecutionPolicy Bypass -File .\test-moss-stream.ps1 -Quick     # debug 跑（仅图编译快）
#
# 默认跑 **release**：交付的 dll 本身就是 release 构建（package.ps1），只有 release 的数字能拿来定
# 超时预算（TLS 在 debug 下慢一到两个量级，拿 debug 数字做决策会误判）。
#
# 探针要验的三件事（都写在断言里）：
#   1. 流式是**多块**返回的（只有一块 = 服务端没真流式，那流式就没收益）
#   2. 音频参数（采样率/声道）**来自响应头**——文档只说"客户端必须根据响应头解释音频参数"，
#      没给默认规格；`from_headers=false` 就说明回落值在撑着，得按打印出来的真值改
#      client.rs 的 FALLBACK_SAMPLE_RATE
#   3. 块是裸 PCM（不带 WAV 容器头）、2 字节对齐
# 另外打印首块耗时，用来和非流式整段的实测 5.3s 对比（流式的收益就在这一个数字上）。
#
# API Key 与音色 ID 默认从本机 settings.json 读取（免手工填 env）；
# 也可先用 $env:MOSS_TTS_API_KEY / $env:MOSS_TTS_VOICE_ID 显式指定。
# 想测长句首块预算：先设 $env:MOSS_PROBE_TEXT = "长文本"。
# 想换模型（如音色不兼容 1.5-flash 时回退）：设 $env:MOSS_TTS_MODEL。
# 注意：本脚本不打印 Key（隐私规则）；dot-source 时环境变量会在 finally 里逐个还原。

param(
    [switch]$Quick
)

# `$ErrorActionPreference` 也是会对调用者生效的全局设置（dot-source 实测会泄出去），
# 与下方环境变量同处理：先存旧值，finally 里换回去。
$PrevEAP = $ErrorActionPreference
$ErrorActionPreference = "Stop"

$ScriptsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = (Resolve-Path (Join-Path $ScriptsDir "..\..")).Path
$PluginDir  = Join-Path $RepoRoot "plugins\engines\moss-tts"
$TargetDir  = Join-Path $RepoRoot "plugins\target"

$PrevKey    = $env:MOSS_TTS_API_KEY
$PrevVoice  = $env:MOSS_TTS_VOICE_ID
$PrevModel  = $env:MOSS_TTS_MODEL
$PrevRevoke = $env:CARGO_HTTP_CHECK_REVOKE
$PrevTargetDir = $env:CARGO_TARGET_DIR
$PrevEncoding  = [Console]::OutputEncoding

# 从取 Key 开始就进 try：dot-source 时 `exit` 不会结束调用者进程（退出码还被吞成 0），
# 明文 Key 与 Stop 偏好会留在调用者会话里；改成 throw 后所有出口都经过 finally。
try {
    Push-Location $PluginDir
    # cargo 打的是 UTF-8，控制台默认按 GBK 解码会把 [probe] 行变成乱码；
    # 实测数字要抄进文档，乱码等于没观测。
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $env:CARGO_HTTP_CHECK_REVOKE = "false"
    $env:CARGO_TARGET_DIR = $TargetDir

    # -- 1. 读本机 settings.json（新位置 plugin_config/plugin_voices 优先，回落遗留字段）--
    $settingsPath = Join-Path $env:APPDATA "com.voiceassist.app\settings.json"
    $obj = $null
    if (Test-Path $settingsPath) {
        $raw = (Get-Content $settingsPath -Raw).TrimStart([char]0xFEFF)
        $obj = $raw | ConvertFrom-Json
    }
    if (-not $env:MOSS_TTS_API_KEY -and $obj) {
        $k = $obj.plugin_config.'moss-tts'.api_key
        if (-not $k) { $k = $obj.moss_api_key }
        if ($k) { $env:MOSS_TTS_API_KEY = $k }
    }
    if (-not $env:MOSS_TTS_VOICE_ID -and $obj) {
        $v = $obj.plugin_voices.'moss-tts'
        if (-not $v) { $v = $obj.moss_voice_id }
        if ($v) { $env:MOSS_TTS_VOICE_ID = $v }
    }
    if (-not $env:MOSS_TTS_API_KEY) {
        Write-Host "[SKIP] 拿不到 MOSS_TTS_API_KEY：本机 settings.json 里没有，也没设环境变量" -ForegroundColor Yellow
        throw "缺少 MOSS_TTS_API_KEY"
    }
    if (-not $env:MOSS_TTS_VOICE_ID) {
        Write-Host "[SKIP] 拿不到 MOSS_TTS_VOICE_ID：本机没保存过 MOSS 音色（语音中心 →「我的音色」）" -ForegroundColor Yellow
        throw "缺少 MOSS_TTS_VOICE_ID"
    }
    Write-Host "[OK] API Key 就绪（长度 $($env:MOSS_TTS_API_KEY.Length)）/ 音色 ID 就绪（$($env:MOSS_TTS_VOICE_ID.Substring(0,8))…）" -ForegroundColor Green

    # -- 2. 跑探针 --
    # 参数拼成显式 [string[]] 再滑传：写成 `$x = if(...){@("a")}` 会被降级成字符串，
    # 滑传后参数静默丢失（mimo 那份踩过）。
    $CargoArgs = [string[]]@("test", "--lib")
    if (-not $Quick) { $CargoArgs += "--release" }
    $CargoArgs += @("--", "--ignored", "--nocapture", "实网探针")
    $Profile_ = if ($Quick) { "debug" } else { "release" }

    # 预检：先 `--list` 数一遍要跑的探针（不触网、秒级）。名称过滤 0 命中时 cargo 仍返 exit 0，
    # 不数一下就会在"探针改了名/过滤写错"时假绿着打印 [DONE]。
    # `--list` 的输出结尾是"N tests, M benchmarks"，没有真跑那行 "running N tests"。
    $Expected = 1
    $ListOut = cargo @([string[]]($CargoArgs + "--list")) 2>&1
    $ListLine = $ListOut | Select-String -Pattern '(\d+) tests?, (\d+) benchmarks' | Select-Object -First 1
    if (-not $ListLine) { throw "预检失败：`--list` 输出里读不到 'N tests, M benchmarks' 计数行（看上方 cargo 报错）" }
    $Ran = [int]$ListLine.Matches[0].Groups[1].Value
    if ($Ran -ne $Expected) {
        throw "预检失败：期望跑 $Expected 只探针，实际匹配到 $Ran 只（探针改名或过滤名称写错）"
    }

    Write-Host "[RUN] moss-tts #[ignore] 实网探针（profile=$Profile_）" -ForegroundColor Cyan
    cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "探针未通过（见上方输出）。若报「音频参数走了回落值」，按打印出的采样率改 client.rs 的 FALLBACK_SAMPLE_RATE；若报 404 voice_not_found，说明该音色不支持当前模型，用 `$env:MOSS_TTS_MODEL 换模型再试"
    }
} finally {
    Pop-Location
    [Console]::OutputEncoding = $PrevEncoding
    $ErrorActionPreference = $PrevEAP
    foreach ($pair in @(
        @('MOSS_TTS_API_KEY', $PrevKey), @('MOSS_TTS_VOICE_ID', $PrevVoice),
        @('MOSS_TTS_MODEL', $PrevModel), @('CARGO_HTTP_CHECK_REVOKE', $PrevRevoke),
        @('CARGO_TARGET_DIR', $PrevTargetDir)
    )) {
        if ($null -ne $pair[1]) { Set-Item -Path "Env:$($pair[0])" -Value $pair[1] }
        else { Remove-Item "Env:$($pair[0])" -ErrorAction SilentlyContinue }
    }
}
Write-Host "[DONE] 探针通过；试听 wav 在 %TEMP%\moss-probe-stream.wav（音调不对=采样率解析错了）" -ForegroundColor Green
