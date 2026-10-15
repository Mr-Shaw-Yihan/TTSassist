// MOSS TTS 插件（Mossland·云端·非流式）——由本体内置引擎下沉而来。
//
// 协议（doc/tts-api/moss-tts.md）：POST https://api.mosi.cn/v1/audio/speech，
// `Authorization: Bearer <key>`，body `{model:"moss-tts", input, voice_id,
// response_format:"mp3", delivery_method:"url"}`；`delivery_method=url` 时同步返回
// JSON 与结果 URL，再 GET 该 URL 取 mp3 字节。接口对请求体做严格字段校验，
// 多一个键就 400，所以 body 只发这五个字段。
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
//
// 输出：MP3（服务端编码，本插件不转码）。

plugin_api::va_tts_plugin! {
    id: "moss-tts",
    name: "MOSS TTS（Mossland）",
    version: "0.1.0",
    audio_format: "mp3",
    // 空表是刻意选择：MOSS 音色 100% 来自宿主「我的音色」，见文件头注释
    voices_json: "[]",
    synthesize: synthesize,
}

/// 合成端点
const ENDPOINT: &str = "https://api.mosi.cn/v1/audio/speech";
/// 模型名（官方 /v1 用单个 model 字段指定模型）
const MODEL: &str = "moss-tts";
/// 音色库地址（用户在此复制 voice_id；错误文案里给出，省得用户找不着）
const VOICE_LIBRARY_URL: &str = "https://mossland.mosi.cn/voice/library";
/// API Key 获取地址
const API_KEY_URL: &str = "https://platform.mosi.cn/app/api-keys";
/// 连接超时：宿主默认合成超时 60s，留出余量让插件先报出可读错误
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// 单请求整体超时（两段式：POST 合成 + GET 下载，各占一次）
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(50);
/// 服务端返回音频 URL 的候选字段名。文档没给示例响应体，本体内置版就是按这几个
/// 名字兜的（含嵌套 data.url）；真机验收必须确认命中哪一个，命中后可收窄成单字段。
const URL_FIELDS: [&str; 5] = ["audio_url", "url", "output_url", "audio", "result_url"];

/// 一次合成请求的全部材料（凭据与请求体一起校验完再发，便于无网络单测）
#[derive(Debug)]
struct Prepared {
    api_key: String,
    body: serde_json::Value,
}

/// 校验入参并组装请求体（纯函数：环境变量由调用方读好后递进来）
fn prepare(
    text: &str,
    voice: Option<&str>,
    raw_key: Option<String>,
) -> Result<Prepared, String> {
    if text.trim().is_empty() {
        return Err("合成文本为空".to_string());
    }
    let api_key = raw_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            format!(
                "未配置 MOSS API Key：请在「语音中心 → MOSS TTS」的插件设置中填写（获取：{API_KEY_URL}）"
            )
        })?;
    // MOSS 没有「服务端默认音色」，voice_id 必填：空值一律拦在发请求之前，
    // 并把音色库地址写进错误里（用户要去那儿复制 voice_id）
    let voice_id = voice
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "default")
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "未选择 MOSS 音色：请在「语音中心 → MOSS TTS」的「我的音色」里保存音色 ID（音色库：{VOICE_LIBRARY_URL}）"
            )
        })?;
    Ok(Prepared {
        api_key,
        body: build_request_body(text, &voice_id),
    })
}

/// 合成请求体：严格只发文档规定的五个字段（多一个未知键服务端就返 400）
fn build_request_body(text: &str, voice_id: &str) -> serde_json::Value {
    serde_json::json!({
        "model": MODEL,
        "input": text,
        "voice_id": voice_id,
        "response_format": "mp3",
        "delivery_method": "url"
    })
}

/// 从响应 JSON 里取音频 URL。
/// 报错只列**字段名**不回显字段值：响应体可能回声用户文本，而错误串会被宿主
/// 显示甚至落日志（隐私口径：用户合成文本不进日志）。
fn extract_audio_url(response: &serde_json::Value) -> Result<String, String> {
    for field in URL_FIELDS {
        if let Some(url) = response[field].as_str() {
            if url.starts_with("http") {
                return Ok(url.to_string());
            }
        }
    }
    if let Some(url) = response["data"]["url"].as_str() {
        if url.starts_with("http") {
            return Ok(url.to_string());
        }
    }
    Err(format!(
        "MOSS 响应中未找到音频 URL（响应字段：{}）。接口可能已变更，请把这段反馈给开发者",
        top_level_keys(response)
    ))
}

/// 响应的顶层字段名清单（只取名不取值，见 `extract_audio_url` 的隐私口径）
fn top_level_keys(response: &serde_json::Value) -> String {
    match response.as_object() {
        Some(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => "<非 JSON 对象>".to_string(),
    }
}

/// 文本 → MP3 字节（voice 必须是宿主「我的音色」里保存的 voice_id）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let prepared = prepare(text, voice, std::env::var("MOSS_TTS_API_KEY").ok())?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;

    // 1) 合成：POST → JSON（delivery_method=url）
    let resp = client
        .post(ENDPOINT)
        .header("Authorization", format!("Bearer {}", prepared.api_key))
        .header("Content-Type", "application/json")
        .json(&prepared.body)
        .send()
        .map_err(|e| format!("请求 MOSS 失败：{e}（请检查网络与 API Key）"))?;
    let status = resp.status();
    if !status.is_success() {
        // 只取状态码与错误结构里的 message，不整体回显（同上隐私口径）
        let body = resp.text().unwrap_or_default();
        return Err(friendly_error(status.as_u16(), &body));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("MOSS 响应 JSON 解析失败: {e}"))?;

    // 2) 下载音频
    let audio_url = extract_audio_url(&json)?;
    let bytes = client
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

/// 非 2xx → 面向用户的中文错误（网关缺 Key 时可能返 403 而非 401，两种都要认）
fn friendly_error(code: u16, body: &str) -> String {
    let suffix = parse_error_message(body)
        .map(|m| format!("：{m}"))
        .unwrap_or_default();
    match code {
        401 | 403 => format!(
            "MOSS API Key 无效或未授权（{code}{suffix}），请在插件设置中检查（获取：{API_KEY_URL}）"
        ),
        400 => format!(
            "MOSS 拒绝了请求（400{suffix}）：接口严格校验入参，请检查音色 ID 是否有效"
        ),
        429 => format!("MOSS 请求过于频繁（429{suffix}），请稍后重试"),
        _ => format!("MOSS 合成失败（HTTP {code}{suffix}）"),
    }
}

/// 取错误体里的服务端 message（官方结构 `{"error":{"message":…}}`，兼容顶层 `{"message":…}`）。
/// 只取这一句而不整体回显：原始错误体可能带着请求回声。
fn parse_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["error"]["message"]
        .as_str()
        .or_else(|| v["message"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(text: &str, voice: Option<&str>, key: Option<&str>) -> serde_json::Value {
        prepare(text, voice, key.map(str::to_string))
            .expect("应组装成功")
            .body
    }

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

    #[test]
    fn 请求体只发文档规定的五个字段() {
        let body = body_of("你好", Some("V1"), Some("K1"));
        assert_eq!(body["model"], "moss-tts");
        assert_eq!(body["input"], "你好");
        assert_eq!(body["voice_id"], "V1");
        assert_eq!(body["response_format"], "mp3");
        assert_eq!(body["delivery_method"], "url");
        // 接口严格校验，多一个未知键就 400 invalid_request_error
        let obj = body.as_object().unwrap();
        assert_eq!(obj.len(), 5, "多余字段会被服务端拒：{body}");
    }

    #[test]
    fn 音色id原样透传并去首尾空白() {
        assert_eq!(body_of("hi", Some(" 11d3a27a-0951 "), Some("K"))["voice_id"], "11d3a27a-0951");
    }

    #[test]
    fn 缺key报中文错并给获取地址() {
        for key in [None, Some(""), Some("   ")] {
            let e = prepare("你好", Some("V1"), key.map(str::to_string)).unwrap_err();
            assert!(e.contains("API Key"), "实际: {e}");
            assert!(e.contains(API_KEY_URL), "应给出获取地址，实际: {e}");
        }
    }

    #[test]
    fn 缺音色报中文错并给音色库地址() {
        // MOSS 没有「服务端默认音色」，voice_id 必填，所以空值必须拦在发请求之前
        for voice in [None, Some(""), Some("   "), Some("default")] {
            let e = prepare("你好", voice, Some("K1".into())).unwrap_err();
            assert!(e.contains("我的音色"), "应指向宿主的音色入口，实际: {e}");
            assert!(e.contains(VOICE_LIBRARY_URL), "应给出音色库地址，实际: {e}");
        }
    }

    #[test]
    fn 空文本直接报错不发请求() {
        let e = prepare("   ", Some("V1"), Some("K1".into())).unwrap_err();
        assert!(e.contains("文本为空"), "实际: {e}");
    }

    #[test]
    fn 音频url抽取覆盖候选字段与嵌套() {
        for f in URL_FIELDS {
            let resp = serde_json::json!({ f: "https://cdn.mosi.cn/a.mp3" });
            assert_eq!(
                extract_audio_url(&resp).unwrap(),
                "https://cdn.mosi.cn/a.mp3",
                "字段 {f} 应被识别"
            );
        }
        // 嵌套形态
        let nested = serde_json::json!({ "data": { "url": "https://cdn.mosi.cn/b.mp3" } });
        assert_eq!(extract_audio_url(&nested).unwrap(), "https://cdn.mosi.cn/b.mp3");
        // 非 http 值不算命中（服务端可能返相对路径或 base64）
        let relative = serde_json::json!({ "url": "/files/a.mp3" });
        assert!(extract_audio_url(&relative).is_err());
    }

    #[test]
    fn 找不到url时报错只列字段名不回显内容() {
        let resp = serde_json::json!({ "data": { "detail": "用户不该被回显的合成文本" } });
        let e = extract_audio_url(&resp).unwrap_err();
        assert!(e.contains("音频 URL"), "实际: {e}");
        assert!(e.contains("data"), "要列出字段名帮助定位，实际: {e}");
        assert!(
            !e.contains("用户不该被回显的合成文本"),
            "错误串会被显示/落日志，不得回显响应内容：{e}"
        );
    }

    #[test]
    fn 常见错误码映射成中文() {
        let e403 = friendly_error(403, r#"{"error":{"message":"permission denied"}}"#);
        assert!(e403.contains("API Key"), "网关缺 Key 常返 403，实际: {e403}");
        let e401 = friendly_error(401, r#"{"error":{"message":"invalid key"}}"#);
        assert!(e401.contains("API Key"), "实际: {e401}");
        let e400 = friendly_error(400, r#"{"error":{"message":"invalid_request_error"}}"#);
        assert!(e400.contains("400"), "实际: {e400}");
        // 错误体里的 message 可以带出来（是服务端文案，不含用户文本）
        assert!(e400.contains("invalid_request_error"), "实际: {e400}");
        // 解析不出结构时不要 panic、也不要空文案
        let plain = friendly_error(500, "Internal Server Error");
        assert!(plain.contains("500"), "实际: {plain}");
    }
}
