# MiMo TTS（流式）实网冒烟：跑 mimo-tts crate 内的 #[ignore] 探针
#
# 用法：
#   pwsh -ExecutionPolicy Bypass -File .\test-mimo-stream.ps1           # 只跑预置音色探针
#   pwsh -ExecutionPolicy Bypass -File .\test-mimo-stream.ps1 -Clone    # 连克隆探针一起跑
#   pwsh -ExecutionPolicy Bypass -File .\test-mimo-stream.ps1 -Clone -Quick   # 改用 debug 跑（仅图编译快）
#
# 默认跑 **release**：交付的 dll 本身就是 release 构建（package.ps1），只有 release 的数字能拿来定超时预算；
# 实测同一只克隆探针 release 要 1.9s、debug 可以在 1.9s ~ 74s 之间乱跳（TLS/base64 在 debug 下慢一到两个量级），
# 拿 debug 数字做决策会误判，所以 -Quick 只用于“先确认能跑通”这类场景。
#
# API Key 与克隆样本默认从本机 settings.json 读取（免手工填 env）；
# 也可先用 $env:MIMO_TTS_API_KEY / $env:VA_CLONE_SAMPLE 显式指定。
# 想测长句首块预算：先设 $env:MIMO_PROBE_TEXT = "长文本"（两个探针都会用这句）。
# 探针输出的 [probe] 行自带 profile（debug/release），拿哪种跑都不会弄混数字。
# dot-source 时若前置条件不满足会抛异常终止你的脚本（故意的：宁可抛，也不要留着明文 Key 往下跑）。
# 注意：本脚本不打印 Key（隐私规则）。

param(
    [switch]$Clone,
    [switch]$Quick
)

# `$ErrorActionPreference` 也是会对调用者生效的全局设置（dot-source 实测会泄出去），
# 与下方环境变量同处理：先存旧值，finally 里换回去。
$PrevEAP = $ErrorActionPreference
$ErrorActionPreference = "Stop"

$ScriptsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = (Resolve-Path (Join-Path $ScriptsDir "..\..")).Path
$PluginDir  = Join-Path $RepoRoot "plugins\engines\mimo-tts"
$TargetDir  = Join-Path $RepoRoot "plugins\target"

# 本脚本要改的环境变量先在此存旧值，退出时在 finally 里逐个恢复（Key 也不能漏）：
# 用 `pwsh -File` 跑时子进程一死就干净了，但 dot-source（`. .\test-mimo-stream.ps1`）
# 会把明文 Key、关掉证书吊销检查等设置长时间留在调用者会话里，后续任何子进程都会继承。
# 控制台上 UTF-8 那一下也是同样性质的全局修改，一并存旧值。
$PrevKey = $env:MIMO_TTS_API_KEY
$PrevCloneSample = $env:VA_CLONE_SAMPLE
$PrevRevoke = $env:CARGO_HTTP_CHECK_REVOKE
$PrevTargetDir = $env:CARGO_TARGET_DIR
$PrevEncoding = [Console]::OutputEncoding

# 从取 Key 开始就进 try：`[SKIP]` 两条早退分支原先放在 try 之外用 `exit 1` 收场，
# 而 dot-source 实测 **脚本里的 exit 并不会结束调用者进程**（调用者接着往下跑，
# 退出码还被吞成 0），于是明文 Key 与 `$ErrorActionPreference = Stop` 照样留在调用者会话里。
# 改成 throw 后，取 Key 之后的所有出口都经过 finally，恢复逻辑只有一份
# （try 之前那几行只是算脚本自身路径与存旧值，本身不会失败）。
try {
    # Push-Location 必须在 try 里：目录不存在时也要走到 finally 的恢复逻辑
    Push-Location $PluginDir
    # cargo 打的是 UTF-8，控制台默认按 GBK 解码会把 [probe] 行变成乱码；
    # 实测数字要抄进文档，乱码等于没观测，故跑前把控制台切到 UTF-8。
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $env:CARGO_HTTP_CHECK_REVOKE = "false"
    $env:CARGO_TARGET_DIR = $TargetDir

    # -- 1. Key：显式 env 优先，否则读本机 settings.json（mimo_api_key 或 plugin_config 里的新位置）--
    if (-not $env:MIMO_TTS_API_KEY) {
        $settingsPath = Join-Path $env:APPDATA "com.voiceassist.app\settings.json"
        if (Test-Path $settingsPath) {
            $raw = (Get-Content $settingsPath -Raw).TrimStart([char]0xFEFF)
            $obj = $raw | ConvertFrom-Json
            $k = $obj.plugin_config.'mimo-tts'.api_key
            if (-not $k) { $k = $obj.mimo_api_key }
            if ($k) { $env:MIMO_TTS_API_KEY = $k }
        }
    }
    if (-not $env:MIMO_TTS_API_KEY) {
        Write-Host "[SKIP] 拿不到 MIMO_TTS_API_KEY：本机 settings.json 里没有，也没设环境变量" -ForegroundColor Yellow
        throw "缺少 MIMO_TTS_API_KEY"
    }
    Write-Host "[OK] API Key 就绪（长度 $($env:MIMO_TTS_API_KEY.Length)）" -ForegroundColor Green

    # -- 2. 克隆样本：仅在 -Clone 时需要 --
    if ($Clone -and -not $env:VA_CLONE_SAMPLE) {
        $settingsPath = Join-Path $env:APPDATA "com.voiceassist.app\settings.json"
        if (Test-Path $settingsPath) {
            $raw = (Get-Content $settingsPath -Raw).TrimStart([char]0xFEFF)
            $rel = ($raw | ConvertFrom-Json).clone_voice_path
            if ($rel) {
                $abs = Join-Path (Join-Path $env:APPDATA "com.voiceassist.app") $rel
                if (Test-Path $abs) { $env:VA_CLONE_SAMPLE = $abs }
            }
        }
    }
    if ($Clone -and -not $env:VA_CLONE_SAMPLE) {
        Write-Host "[SKIP] -Clone 需要克隆样本，但本机没导入过（或文件已删）" -ForegroundColor Yellow
        throw "缺少克隆样本 VA_CLONE_SAMPLE"
    }

    # -- 3. 跑探针 --
    # 探针是两个 #[ignore] 的独立测试，必须按 -Clone 选着跑：不加名称过滤就会连克隆探针一起跑，
    # 而克隆探针没有样本会直接 panic，于是“只测预置”这种最常用的跑法必然红（实测踩过）。
    # 参数拼成显式 [string[]] 再滑传：写成 `$x = if(...){@("a")}` 会被降级成字符串，
    # 滑传后参数静默丢失（实测踩过：过滤条件看起来完全没生效）。
    $CargoArgs = [string[]]@("test", "--lib")
    if (-not $Quick) { $CargoArgs += "--release" }
    $CargoArgs += @("--", "--ignored", "--nocapture")
    if (-not $Clone) { $CargoArgs += "实网探针_预置" }
    $Profile_ = if ($Quick) { "debug" } else { "release" }

    # 预检：先 `--list` 数一遍要跑的探针（不触网、秒级）。名称过滤 0 命中时 cargo 仍返 exit 0，
    # 不数一下就会在“探针改了名 / 过滤写错”时假绿着打印 [DONE]。
    # 注意 `--list` 的输出格式与真跑不同：结尾是“N tests, M benchmarks”，
    # **没有**真跑那行 “running N tests”（拿那个模式来解析会永远读不到，实测踩过）。
    # 也不锚定行首：cargo 自己的 “     Running unittests ...” 行带缩进，两种行都可能在变。
    $Expected = if ($Clone) { 2 } else { 1 }
    $ListOut = cargo @([string[]]($CargoArgs + "--list")) 2>&1
    $ListLine = $ListOut | Select-String -Pattern '(\d+) tests?, (\d+) benchmarks' | Select-Object -First 1
    if (-not $ListLine) { throw "预检失败：`--list` 输出里读不到 'N tests, M benchmarks' 计数行（看上方 cargo 报错）" }
    $Ran = [int]$ListLine.Matches[0].Groups[1].Value
    if ($Ran -ne $Expected) {
        throw "预检失败：期望跑 $Expected 只探针，实际匹配到 $Ran 只（探针改名或过滤名称写错）"
    }

    Write-Host "[RUN] mimo-tts #[ignore] 实网探针（Clone=$Clone / profile=$Profile_ / 应跑 $Expected 只）" -ForegroundColor Cyan
    cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) { throw "探针未通过（见上方输出）。首块若报 RIFF 未剥离，检查 stream.rs worker 首块处理" }
} finally {
    Pop-Location
    [Console]::OutputEncoding = $PrevEncoding
    $ErrorActionPreference = $PrevEAP
    if ($null -ne $PrevKey) { $env:MIMO_TTS_API_KEY = $PrevKey } else { Remove-Item Env:MIMO_TTS_API_KEY -ErrorAction SilentlyContinue }
    if ($null -ne $PrevCloneSample) { $env:VA_CLONE_SAMPLE = $PrevCloneSample } else { Remove-Item Env:VA_CLONE_SAMPLE -ErrorAction SilentlyContinue }
    if ($null -ne $PrevRevoke) { $env:CARGO_HTTP_CHECK_REVOKE = $PrevRevoke } else { Remove-Item Env:CARGO_HTTP_CHECK_REVOKE -ErrorAction SilentlyContinue }
    if ($null -ne $PrevTargetDir) { $env:CARGO_TARGET_DIR = $PrevTargetDir } else { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
}
Write-Host "[DONE] 探针通过；试听 wav 在 %TEMP%\mimo-probe-*.wav" -ForegroundColor Green
