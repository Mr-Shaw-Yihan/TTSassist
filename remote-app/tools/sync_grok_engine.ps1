# 同步 grok 引擎源码到 remote-app/assets/grok/（logo WebView 加载）。
# 构建前跑一次；改引擎后重跑。grok 素材为演示占位，换原创素材时只替换本目录文件
# （见 doc/移动端遥控器设计.md §四 占位声明）。
$ErrorActionPreference = 'stop'
$engine = Resolve-Path (Join-Path $PSScriptRoot '..\..\src\components\FloatingBall\engine')
$out = Join-Path $PSScriptRoot '..\assets\grok'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$files = @('geometry-data.js','math.js','tables.js','pose.js','tricks.js','fx.js','eyes.js','character.js')
foreach ($f in $files) {
    Copy-Item (Join-Path $engine $f) $out -Force
    Write-Host "sync $f"
}
Write-Host "done: $out"
