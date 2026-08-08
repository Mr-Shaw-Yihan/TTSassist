// HTTP 客户端：两部分——
// 1. 引导期文件下载（Python 运行时等，reqwest blocking 直接下到磁盘）
// 2. 与 genie_server.py 的 API 通信（health / ensure_resources / load_character / tts）

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// 引导期下载共用的 HTTP 客户端
fn http() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("VoiceAssist-GeniePlugin")
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("初始化网络客户端失败: {e}"))
}

/// 下载文件到指定路径（流式写盘）。
/// progress：(已下载字节, 总字节 Option)——服务端不给 Content-Length 时 total 为 None
pub fn download_file_with_progress(
    url: &str,
    dest: &Path,
    progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<(), String> {
    let client = http()?;
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("下载失败（{url}）: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败（HTTP {}）: {url}", resp.status()));
    }
    let total = resp.content_length();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut resp = resp;
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = resp
            .read(&mut buf[..])
            .map_err(|e| format!("下载中断（可重试）: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入磁盘失败（可能空间不足）: {e}"))?;
        done += n as u64;
        if let Some(cb) = progress {
            cb(done, total);
        }
    }
    Ok(())
}

// ── 服务端 API ─────────────────────────────────────────

fn api_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("初始化网络客户端失败: {e}"))
}

/// 健康检查（2 秒超时，用于探活）
pub fn health(port: u16) -> bool {
    let Ok(client) = api_client(2) else { return false };
    client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// 确保 Genie 运行资源（GenieData + RoBERTa，首次约 400MB，服务端幂等）
pub fn ensure_resources(port: u16) -> Result<(), String> {
    let client = api_client(1800)?;
    let resp = client
        .post(format!("http://127.0.0.1:{port}/ensure_resources"))
        .json(&serde_json::json!({}))
        .send()
        .map_err(|e| format!("资源准备请求失败: {e}"))?;
    if resp.status().is_success() {
        return Ok(());
    }
    let detail = error_detail(resp);
    Err(format!(
        "Genie 运行资源下载失败（首次约 400MB，请保持联网后重试）: {detail}"
    ))
}

/// 加载音色（预置音色缺失时服务端会自动下载，故超时给足）
pub fn load_character(port: u16, voice_id: &str) -> Result<(), String> {
    let client = api_client(900)?;
    let resp = client
        .post(format!("http://127.0.0.1:{port}/load_character"))
        .json(&serde_json::json!({ "voice_id": voice_id }))
        .send()
        .map_err(|e| format!("加载音色请求失败: {e}"))?;
    if resp.status().is_success() {
        return Ok(());
    }
    let detail = error_detail(resp);
    Err(format!("加载音色失败: {detail}"))
}

/// 卸载内存中的音色（释放权重与参考音频，不删磁盘文件）。
/// 服务端幂等：未加载的音色也返回成功。短超时（纯内存操作）。
pub fn unload_character(port: u16, voice_id: &str) -> Result<(), String> {
    let client = api_client(30)?;
    let resp = client
        .post(format!("http://127.0.0.1:{port}/unload_character"))
        .json(&serde_json::json!({ "voice_id": voice_id }))
        .send()
        .map_err(|e| format!("卸载音色请求失败: {e}"))?;
    if resp.status().is_success() {
        return Ok(());
    }
    let detail = error_detail(resp);
    Err(format!("卸载音色失败: {detail}"))
}

/// 合成：文本 → 完整 WAV 字节
pub fn tts(port: u16, voice_id: &str, text: &str) -> Result<Vec<u8>, String> {
    let client = api_client(300)?;
    let resp = client
        .post(format!("http://127.0.0.1:{port}/tts"))
        .json(&serde_json::json!({ "voice_id": voice_id, "text": text }))
        .send()
        .map_err(|e| format!("合成请求失败: {e}"))?;
    if !resp.status().is_success() {
        let detail = error_detail(resp);
        return Err(format!("语音合成失败: {detail}"));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("读取合成音频失败: {e}"))?;
    if bytes.is_empty() {
        return Err("语音合成未返回音频".into());
    }
    Ok(bytes.to_vec())
}

/// 从 FastAPI 错误响应中提取 detail 字段（拿不到就用状态码）
fn error_detail(resp: reqwest::blocking::Response) -> String {
    let status = resp.status();
    match resp.json::<serde_json::Value>() {
        Ok(v) => v
            .get("detail")
            .map(|d| d.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|| format!("HTTP {status}")),
        Err(_) => format!("HTTP {status}"),
    }
}
