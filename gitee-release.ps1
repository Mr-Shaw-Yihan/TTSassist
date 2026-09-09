# Gitee Release API 公共函数 —— 本体发布与遥控 App 发布共用，务必不要在多个脚本里各存一份。
# 用法（dot-source 后函数即进入调用方作用域）：
#   . (Join-Path $RepoRoot 'gitee-release.ps1')
#
# 三条硬教训（v1.8.4 发版实测得出，不要退回 curl.exe）：
#   1. curl --data-urlencode 走 form-urlencoded 且不带 charset，Gitee 按 Latin-1 存入 →
#      中文标题/说明变成 00E9 0081 00A5 那类二次编码乱码。含中文的内容必须走 JSON + 显式
#      charset=utf-8。
#   2. curl 的 UTF-8 响应会被 PowerShell 5.1 按控制台代码页（GBK）解码，中文续字节吞掉引号 →
#      ConvertFrom-Json 抛 ArgumentException（表现为"Release 建好了但附件没传"）。响应必须由
#      .NET 自行按 charset 解码。
#   3. 附件字段名是 file（不是 files[]/files）；access_token 只能走查询参数——放进 multipart
#      分块（尤其带 Content-Type）会被当成一个文件，表现为 401 / code 40001 / "file is missing"。
#
# 另注：Gitee 的 API 读接口与 raw 都有缓存，页面即时生效。脚本内不要用 API 回读当判据，
# 更不要据此重复上传。
#
# 本文件须保持 UTF-8 带 BOM（含中文注释，PS 5.1 无 BOM 会按 GBK 误读导致解析失败）。

Add-Type -AssemblyName System.Net.Http

<#
.SYNOPSIS
  从 git remote URL 解析 owner/repo（兼容 https/ssh/scp-like 三种形态）。
#>
function Parse-OwnerRepo {
    param([string]$Url)
    $n = ($Url -replace '\.git$', '' -replace '^[a-z]+://[^/]+/', '' -replace '^[^@]+@[^:]+:', '')
    $p = $n.Split('/')
    if ($p.Count -lt 2) { throw "无法从远端 URL 解析 owner/repo：$Url" }
    return @{ Owner = $p[-2]; Repo = $p[-1] }
}

<#
.SYNOPSIS
  Gitee API 统一出入口：请求体用 UTF-8 字节的 JSON 并声明 charset，响应由 .NET 按 charset 解码。
#>
function Invoke-GiteeJson {
    param([string]$Method, [string]$Uri, [hashtable]$Body)
    $client = New-Object System.Net.Http.HttpClient
    try {
        $client.Timeout = [TimeSpan]::FromMinutes(2)
        $req = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]$Method, $Uri)
        if ($Body) {
            $json = $Body | ConvertTo-Json -Depth 5
            $req.Content = New-Object System.Net.Http.StringContent($json, [System.Text.Encoding]::UTF8, 'application/json')
        }
        $resp = $client.SendAsync($req).Result
        $text = $resp.Content.ReadAsStringAsync().Result
        if (-not $resp.IsSuccessStatusCode) { throw "HTTP $([int]$resp.StatusCode) : $text" }
        if ($text) { return ($text | ConvertFrom-Json) }
        return $null
    } finally { $client.Dispose() }
}

<#
.SYNOPSIS
  上传 Release 附件（multipart，字段名 file，令牌走查询参数）。
#>
function Send-GiteeAttachment {
    param([string]$Uri, [string]$FilePath, [string]$ContentType = 'application/octet-stream')
    $bytes = [System.IO.File]::ReadAllBytes($FilePath)
    $client = New-Object System.Net.Http.HttpClient
    try {
        $client.Timeout = [TimeSpan]::FromMinutes(20)
        $mp = New-Object System.Net.Http.MultipartFormDataContent
        $fc = New-Object System.Net.Http.ByteArrayContent(, $bytes)
        $fc.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse($ContentType)
        $mp.Add($fc, 'file', [System.IO.Path]::GetFileName($FilePath))
        $resp = $client.PostAsync($Uri, $mp).Result
        $text = $resp.Content.ReadAsStringAsync().Result
        if (-not $resp.IsSuccessStatusCode) { throw "HTTP $([int]$resp.StatusCode) : $text" }
        return $text
    } finally { $client.Dispose() }
}

<#
.SYNOPSIS
  按 tag 查 Gitee Release id；不存在返回 $null。
.DESCRIPTION
  先试 tags 端点，再回退列表检索——tags 端点偶发返回不含 id 的响应，只查它会误判成"不存在"
  进而重复建 Release。
#>
function Get-GiteeReleaseIdByTag {
    param([string]$ApiBase, [string]$Token, [string]$Tag)
    $q = "?access_token=$Token"
    try {
        $one = Invoke-GiteeJson -Method 'GET' -Uri "$ApiBase/releases/tags/$Tag$q"
        if ($one -and $one.id) { return $one.id }
    } catch { }
    $all = @(Invoke-GiteeJson -Method 'GET' -Uri "$ApiBase/releases$q&per_page=100")
    $hit = $all | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1
    if ($hit) { return $hit.id }
    return $null
}

<#
.SYNOPSIS
  建 Gitee Release（标题/说明含中文也安全），返回 id。
#>
function New-GiteeRelease {
    param(
        [string]$ApiBase, [string]$Token, [string]$Tag, [string]$Name, [string]$Body,
        [string]$TargetCommitish = 'main'
    )
    $created = Invoke-GiteeJson -Method 'POST' -Uri "$ApiBase/releases?access_token=$Token" -Body @{
        tag_name         = $Tag
        name             = $Name
        body             = $Body
        target_commitish = $TargetCommitish
    }
    if (-not $created -or -not $created.id) { throw 'Gitee 建 Release 失败（响应无 id）' }
    return $created.id
}

<#
.SYNOPSIS
  确保 Release 带上指定附件；已存在同名附件则跳过并返回 $false（替换需先在网页删除后重跑）。
.OUTPUTS
  $true = 本次真的上传了；$false = 已有同名附件、未上传
#>
function Add-GiteeAttachmentIfMissing {
    param(
        [string]$ApiBase, [string]$Token, $ReleaseId, [string]$FilePath,
        [string]$ContentType = 'application/octet-stream'
    )
    $q = "?access_token=$Token"
    $assetName = [System.IO.Path]::GetFileName($FilePath)
    $assets = @(Invoke-GiteeJson -Method 'GET' -Uri "$ApiBase/releases/$ReleaseId/attach_files$q")
    if (@($assets | Where-Object { $_.name -eq $assetName }).Count -gt 0) { return $false }
    $null = Send-GiteeAttachment -Uri "$ApiBase/releases/$ReleaseId/attach_files$q" -FilePath $FilePath -ContentType $ContentType
    return $true
}
