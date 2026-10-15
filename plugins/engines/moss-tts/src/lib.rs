// MOSS TTS 插件（Mossland·云端·流式优先）——由本体内置引擎下沉而来。
//
// 主路径是流式：宿主只要看到 va_tts_stream_* 三符号就直接走流式（没有开关，
// 与 fish/mimo 同一口径）。同时导出阻塞合成，那是**老宿主兼容面**（不带流式 ABI 的
// 宿主加载它能当普通非流式插件用），本期新宿主不会调用它。
//
// 协议（核实自线上文档 platform.mosi.cn/docs/reference/speech，2026-10-15；
// 本地 doc/tts-api/moss-tts.md 是 7 月快照、已过时）：
// - 端点 `POST https://api.mosi.cn/v1/audio/speech`，`Authorization: Bearer <key>`；
// - 流式：`stream:true` + `response_format:"pcm"` + `stream_format:"audio"` → 裸 PCM 分块，
//   **音频参数（采样率/声道）由响应头给出**，客户端自己解释；不得带 delivery_method/sample_rate；
// - 非流式：`response_format:"mp3"` + `delivery_method:"url"` → JSON 的 `url` 字段是音频地址，再 GET 下载；
// - 流式要求模型 `moss-tts-1.5-flash`；模型可经 `MOSS_TTS_MODEL` 覆盖。
// - 接口严格校验入参，多一个未知字段就 400。
//
// 音色（关键差异，别照 mimo/edge 想当然）：MOSS **没有**插件自带音色表——它的音色
// 就是用户从 Mossland 音色库页面复制的 voice_id（UUID），历来由用户手填。宿主
// 「我的音色」（plugin_custom_voices）保存后，选中的 id 经 voice 参数原样透传，
// 与 fish 的克隆 reference_id 同一通道。因此本插件 `voices_json` 恒为空表：
// 硬编一个 UUID 只会陈旧（本体预置的「曼波」id 就已经被换过一次），陈旧后用户
// 看到的是「选了内置音色却合成失败」。
//
// 环境变量（宿主按 manifest.config 声明注入）：
//   MOSS_TTS_API_KEY   必填，platform.mosi.cn → API Key 管理平台创建
//   MOSS_TTS_MODEL     可选，留空即 moss-tts-1.5-flash
//
// 输出：流式 PCM s16le（采样率/声道按响应头）；阻塞 MP3。

mod client;
mod stream;

plugin_api::va_tts_plugin! {
    id: "moss-tts",
    name: "MOSS TTS（Mossland）",
    version: "0.2.0",
    audio_format: "mp3",
    // 空表是刻意选择：MOSS 音色 100% 来自宿主「我的音色」，见文件头注释
    voices_json: "[]",
    synthesize: synthesize,
}

plugin_api::va_tts_stream_plugin! {
    start: stream_start,
    next: stream_next,
    stop: stream_stop,
}

/// 阻塞路径的连接超时（流式路径的超时在 stream.rs 里，含首块/块间两套窗口）
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// 阻塞路径的单请求整体超时。两段式（POST 合成 + GET 下载），manifest 已把宿主
/// 超时放宽到 120s，让插件自己先报出可读错误
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(50);

/// 文本 → MP3 字节（老宿主兼容面；新宿主走流式）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let req = client::request_from_env(text, voice)?;
    let body = client::build_request_body(&req, text);
    let http = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;

    // 1) 合成：POST → JSON（delivery_method=url）
    let resp = http
        .post(client::ENDPOINT)
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("请求 MOSS 失败：{e}（请检查网络与 API Key）"))?;
    let status = resp.status();
    if !status.is_success() {
        // 只取状态码与错误结构里的 message，不整体回显（响应体可能回声用户文本）
        let text = resp.text().unwrap_or_default();
        return Err(client::friendly_error(status.as_u16(), &text));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("MOSS 响应 JSON 解析失败: {e}"))?;

    // 2) 下载音频
    let audio_url = client::extract_audio_url(&json)?;
    let bytes = http
        .get(&audio_url)
        .send()
        .map_err(|e| format!("下载 MOSS 音频失败: {e}"))?
        .bytes()
        .map_err(|e| format!("读取 MOSS 音频字节失败: {e}"))?;
    if bytes.is_empty() {
        return Err("MOSS 返回空音频".to_string());
    }
    Ok(bytes.to_vec())
}

/// 打开流式会话：返回 (音频块格式自述 JSON, 会话句柄)。
/// 采样率/声道来自响应头（文档要求客户端自行解释），所以必须先发请求、读到头才能回 info。
fn stream_start(text: &str, voice: Option<&str>) -> Result<(String, u64), String> {
    let req = client::request_from_env(text, voice)?;
    let body = client::build_stream_request_body(&req, text);
    let session = stream::StreamSession::open(req.api_key, body)?;
    let info = client::stream_info_json(&session.params);
    let id = stream::global_sessions().insert(session);
    Ok((info, id))
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
    fn 内置音色表恒为空() {
        // 守护「音色不硬编在插件里」这条决定：宏把字面量内联成 crate 根的
        // static VA__VOICES（尾部带一个 NUL）。谁往里塞了一个 UUID，这条就红——
        // 本体预置的曼波 id 陈旧过一次，硬编音色只会让用户撞到「选了却合成失败」
        let exported = std::str::from_utf8(crate::VA__VOICES)
            .unwrap()
            .trim_end_matches('\0');
        assert_eq!(exported, "[]");
        let list: Vec<plugin_api::VoiceItem> = serde_json::from_str(exported).unwrap();
        assert!(list.is_empty());
    }
}
