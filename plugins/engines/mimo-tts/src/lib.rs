// MiMo TTS 插件（小米·云端·流式）——由本体内置引擎下沉而来。
//
// 主路径是流式：宿主只要看到 va_tts_stream_* 三符号就直接走流式（没有开关、
// 也没有同引擎整段版回落）。同时导出阻塞合成，那是**老宿主兼容面**（不带流式
// ABI 的宿主加载它能当普通非流式插件用），本期新宿主不会调用它。
//
// 克隆音色：官方低延迟流式未上线时由服务端自动退化为兼容模式（一次性返回），
// 客户端不写降级分支——插件对预置与克隆发同一套请求，拿到几块就播几块。
// 「暂不支持低延迟」写在 voices.label 里由插件自述，宿主与前端零感知。
//
// 配置（宿主按 manifest.config 注入环境变量）：
//   MIMO_TTS_API_KEY   API Key（必填）
//   MIMO_TTS_MODEL     合成模型（留空默认 mimo-v2.5-tts）
//   VA_CLONE_SAMPLE    克隆样本绝对路径（宿主通用机制注入，非 MiMo 专属）

mod client;
mod stream;

pub use client::stream_info_json;

plugin_api::va_tts_plugin! {
    id: "mimo-tts",
    name: "MiMo TTS（小米·流式）",
    version: "0.1.0",
    audio_format: "wav",
    // 静态音色表：克隆项恒在——若按"样本文件是否存在"条件挂载，会出现
    // "看不到入口 → 永远导不进样本"的死锁；未导入就选克隆由插件返回中文错
    voices_json: r#"[{"id":"mimo_default","label":"默认 (mimo_default)"},{"id":"冰糖","label":"冰糖（女声）"},{"id":"茉莉","label":"茉莉（女声）"},{"id":"苏打","label":"苏打（男声）"},{"id":"白桦","label":"白桦（男声）"},{"id":"clone","label":"克隆：我的音色（暂不支持低延迟）"}]"#,
    synthesize: synthesize,
}

plugin_api::va_tts_stream_plugin! {
    start: stream_start,
    next: stream_next,
    stop: stream_stop,
}

/// 阻塞合成（老宿主兼容面）：一次成段 wav
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let api_key = client::read_api_key()?;
    let (model, v) = client::resolve_voice(voice)?;
    let body = client::build_request_body(&model, text, v.as_deref(), None);
    let resp: serde_json::Value = reqwest::blocking::Client::new()
        .post(client::ENDPOINT)
        .header("api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("请求 MiMo 失败：{e}（请检查网络与 API Key）"))?
        .error_for_status()
        .map_err(|e| format!("MiMo 返回错误状态: {e}"))?
        .json()
        .map_err(|e| format!("MiMo 响应 JSON 解析失败: {e}"))?;
    client::extract_audio_data(&resp)
}

/// 打开流式会话：返回 (音频块格式自述 JSON, 会话句柄)
fn stream_start(text: &str, voice: Option<&str>) -> Result<(String, u64), String> {
    let api_key = client::read_api_key()?;
    let (model, v) = client::resolve_voice(voice)?;
    let body = client::build_stream_request_body(&model, text, v.as_deref(), None);
    let session = stream::StreamSession::open(api_key, body)?;
    let id = stream::global_sessions().insert(session);
    Ok((client::stream_info_json(), id))
}

/// 拉下一个 PCM 块（Ok 空 vec = 流正常结束）
fn stream_next(session: u64) -> Result<Vec<u8>, String> {
    stream::global_sessions().next_chunk(session)
}

/// 中止并释放会话（幂等）
fn stream_stop(session: u64) {
    stream::global_sessions().abort(session);
}

#[cfg(test)]
mod tests {
    #[test]
    fn 音色表_克隆项恒在且带提示() {
        // voices_json 由宏内联成静态串，这里重抄一份常量口径做断言
        let raw = r#"[{"id":"mimo_default","label":"默认 (mimo_default)"},{"id":"冰糖","label":"冰糖（女声）"},{"id":"茉莉","label":"茉莉（女声）"},{"id":"苏打","label":"苏打（男声）"},{"id":"白桦","label":"白桦（男声）"},{"id":"clone","label":"克隆：我的音色（暂不支持低延迟）"}]"#;
        let list: Vec<plugin_api::VoiceItem> = serde_json::from_str(raw).unwrap();
        assert_eq!(list.len(), 6);
        let clone = list.iter().find(|v| v.id == "clone").expect("克隆项必须恒在");
        assert!(clone.label.contains("暂不支持低延迟"), "实际: {}", clone.label);
    }
}
