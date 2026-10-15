// MOSS 协议层：请求体 / 响应解析 / 流式音频参数 / 错误文案。
//
// 全部为不碰网络的纯函数（Key 与模型由调用方读好环境变量后递进来），便于无网络单测。
// 协议事实核实自线上文档（2026-10-15 抓取，本地 doc/tts-api/moss-tts.md 是 7 月快照、已过时）：
// - POST https://api.mosi.cn/v1/audio/speech，鉴权 `Authorization: Bearer <key>`；
// - 接口对请求体做**严格字段校验**，未知字段直接 400 invalid_request_error；
// - 非流式：`response_format:"mp3"` + `delivery_method:"url"` → JSON 里的 `url` 字段是音频地址；
// - 流式：`stream:true` + `response_format:"pcm"` + `stream_format:"audio"`，
//   **不得**带 `sample_rate` / `delivery_method` / `file_id`（否则 400 unsupported_for_stream），
//   音频参数（采样率/声道）由**响应头**给出，客户端自己解释；
// - 流式要求模型 `moss-tts-1.5-flash`（不满足报 400 unsupported_stream）。

/// 合成端点（阻塞与流式同一个）
pub const ENDPOINT: &str = "https://api.mosi.cn/v1/audio/speech";
/// 默认模型：文档枚举里的稳定 id，也是唯一支持流式的模型。
/// 可经 `MOSS_TTS_MODEL` 覆盖（官方上新档位或音色不兼容时，用户改字符串即可，不用发插件版）。
/// 注：本体内置版发的 `"moss-tts"` 不在文档枚举里（未文档化别名），已弃用。
pub const DEFAULT_MODEL: &str = "moss-tts-1.5-flash";
/// 音色库地址（用户在此复制 voice_id；错误文案里给出，省得用户找不着）
pub const VOICE_LIBRARY_URL: &str = "https://mossland.mosi.cn/voice/library";
/// API Key 获取地址
pub const API_KEY_URL: &str = "https://platform.mosi.cn/app/api-keys";
/// 响应头里读不到音频参数时的回落采样率。
/// 文档只说"客户端必须根据响应头解释音频参数"、没给默认规格，所以正常路径永远走响应头；
/// 这个回落值取**实测真值**：2026-10-15 实网探针（`scripts/test-moss-stream.ps1`）
/// 读到 `采样率 48000 / 声道 1 / 来自响应头 true`。写 24000 那种"常见 TTS 默认"会在
/// 网关哪天不带头时变成花栗鼠音，比报错更难查。
pub const FALLBACK_SAMPLE_RATE: u32 = 48_000;
/// 回落声道数（实测同为单声道）
pub const FALLBACK_CHANNELS: u32 = 1;
/// 服务端返回音频 URL 的候选字段名。文档的响应示例已确认是 `url`，其余是本体内置版
/// 留下的兜底（不同网关版本可能换名），保留不碍事。
const URL_FIELDS: [&str; 5] = ["url", "audio_url", "output_url", "audio", "result_url"];

/// 一次请求的凭据与目标（校验通过后才有）
#[derive(Debug, PartialEq)]
pub struct Request {
    pub api_key: String,
    pub model: String,
    pub voice_id: String,
}

/// 流式音频参数（采样率 + 声道；位深固定按 16bit 小端解释，宿主只认 pcm_s16le）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioParams {
    pub sample_rate: u32,
    pub channels: u32,
    /// true = 从响应头解析而来；false = 头里没有，用了回落值
    pub from_headers: bool,
}

/// 从环境变量（宿主按 manifest.config 注入）组装请求材料。
/// 阻塞与流式两条路径共用，避免两处各读一遍 env 而口径漂移。
pub fn request_from_env(text: &str, voice: Option<&str>) -> Result<Request, String> {
    resolve_request(
        text,
        voice,
        std::env::var("MOSS_TTS_API_KEY").ok(),
        std::env::var("MOSS_TTS_MODEL").ok().as_deref(),
    )
}

/// API Key 校验（纯函数）：缺失/空白 → 中文错误并附获取地址
pub fn api_key_from(raw: Option<String>) -> Result<String, String> {
    raw.map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            format!(
                "未配置 MOSS API Key：请在「语音中心 → MOSS TTS」的插件设置中填写（获取：{API_KEY_URL}）"
            )
        })
}

/// 模型解析（纯函数）：空/未设 → `DEFAULT_MODEL`，有值 → 去空白后原样
pub fn resolve_model(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string()
}

/// 音色解析（纯函数）：MOSS 没有"服务端默认音色"，voice_id 必填。
/// 宿主「我的音色」保存的 id 经 voice 参数原样透传。
pub fn resolve_voice_id(voice: Option<&str>) -> Result<String, String> {
    voice
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "default")
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "未选择 MOSS 音色：请在「语音中心 → MOSS TTS」的「我的音色」里保存并选用音色 ID（音色库：{VOICE_LIBRARY_URL}）"
            )
        })
}

/// 校验入参并解析出请求材料（纯函数：文本非空 + Key + 音色 + 模型）
pub fn resolve_request(
    text: &str,
    voice: Option<&str>,
    raw_key: Option<String>,
    raw_model: Option<&str>,
) -> Result<Request, String> {
    if text.trim().is_empty() {
        return Err("合成文本为空".to_string());
    }
    Ok(Request {
        api_key: api_key_from(raw_key)?,
        model: resolve_model(raw_model),
        voice_id: resolve_voice_id(voice)?,
    })
}

/// 非流式请求体：`delivery_method:"url"` → 响应 JSON 的 `url` 是音频地址
pub fn build_request_body(req: &Request, text: &str) -> serde_json::Value {
    serde_json::json!({
        "model": req.model,
        "input": text,
        "voice_id": req.voice_id,
        "response_format": "mp3",
        "delivery_method": "url"
    })
}

/// 流式请求体：`stream:true` + `response_format:"pcm"` + `stream_format:"audio"`。
/// **绝不能带** `delivery_method` / `sample_rate` / `file_id`——服务端严格校验，多一个键就 400。
pub fn build_stream_request_body(req: &Request, text: &str) -> serde_json::Value {
    serde_json::json!({
        "model": req.model,
        "input": text,
        "voice_id": req.voice_id,
        "stream": true,
        "response_format": "pcm",
        "stream_format": "audio"
    })
}

/// 从非流式响应里取音频 URL。
/// 报错只列**字段名**不回显字段值：响应体可能回声用户文本，而错误串会被宿主
/// 显示甚至落日志（隐私口径：用户合成文本不进日志）。
pub fn extract_audio_url(response: &serde_json::Value) -> Result<String, String> {
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
pub fn top_level_keys(response: &serde_json::Value) -> String {
    match response.as_object() {
        Some(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
        }
        _ => "<非 JSON 对象>".to_string(),
    }
}

/// 从流式响应头解析音频参数（纯函数：名字统一按小写比较）。
/// 认多种写法：`content-type: audio/L16;rate=24000;channels=1`、裸 `rate=`、
/// `x-sample-rate` / `x-audio-sample-rate` / `sample-rate`、`x-channels` 等；
/// 一个都读不到就用回落值（`from_headers=false`）。
pub fn parse_audio_params<'a, I>(headers: I) -> AudioParams
where
    I: Iterator<Item = (&'a str, &'a str)>,
{
    let mut rate: Option<u32> = None;
    let mut channels: Option<u32> = None;
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if name == "content-type" {
            // 裸 PCM 的标准写法是 audio/L16;rate=…;channels=…（RFC 4856 风格）
            let lower = value.to_ascii_lowercase();
            if rate.is_none() {
                rate = param_after_eq(&lower, "rate");
            }
            if channels.is_none() {
                channels = param_after_eq(&lower, "channels");
            }
            continue;
        }
        if rate.is_none()
            && matches!(
                name.as_str(),
                "x-sample-rate" | "x-audio-sample-rate" | "sample-rate" | "audio-sample-rate"
                    | "x-samplerate" | "x-rate"
            )
        {
            rate = value.parse().ok();
        }
        if channels.is_none()
            && matches!(
                name.as_str(),
                "x-channels" | "x-audio-channels" | "channels" | "audio-channels"
            )
        {
            channels = value.parse().ok();
        }
    }
    // 离谱值不采信：采样率错了是"花栗鼠/慢放"、声道错了是杂音，都比回落更难排查
    let rate = rate.filter(|r| (8_000..=192_000).contains(r));
    let channels = channels.filter(|c| (1..=8).contains(c));
    AudioParams {
        from_headers: rate.is_some() || channels.is_some(),
        sample_rate: rate.unwrap_or(FALLBACK_SAMPLE_RATE),
        channels: channels.unwrap_or(FALLBACK_CHANNELS),
    }
}

/// 从 `;` 分隔的参数串里取 `key=<数字>`（`haystack` 需已小写）
fn param_after_eq(haystack: &str, key: &str) -> Option<u32> {
    let needle = format!("{key}=");
    let start = haystack.find(&needle)? + needle.len();
    let rest = &haystack[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// 宿主拉块通道的格式自述（`va_tts_stream_start` 返回给宿主的第一项）
pub fn stream_info_json(params: &AudioParams) -> String {
    format!(
        r#"{{"format":"pcm_s16le","sample_rate":{},"channels":{}}}"#,
        params.sample_rate, params.channels
    )
}

/// 把任意长度字节流切成 2 字节对齐的块（s16le 帧边界）：
/// 余下的奇数尾巴存进 `carry`，与下一块拼接。
pub fn align_bytes(carry: &mut Option<u8>, bytes: &[u8]) -> Vec<u8> {
    if carry.is_none() && bytes.len() % 2 == 0 {
        return bytes.to_vec();
    }
    let mut buf = Vec::with_capacity(bytes.len() + 1);
    if let Some(prev) = carry.take() {
        buf.push(prev);
    }
    buf.extend_from_slice(bytes);
    let aligned = buf.len() & !1;
    if aligned < buf.len() {
        *carry = Some(buf[aligned]);
    }
    buf.truncate(aligned);
    buf
}

/// 非 2xx → 面向用户的中文错误（网关缺 Key 时可能返 403 而非 401，两种都要认）
pub fn friendly_error(code: u16, body: &str) -> String {
    let suffix = parse_error_message(body)
        .map(|m| format!("：{m}"))
        .unwrap_or_default();
    match code {
        401 | 403 => format!(
            "MOSS API Key 无效或未授权（{code}{suffix}），请在插件设置中检查（获取：{API_KEY_URL}）"
        ),
        404 => format!(
            "MOSS 报音色不存在（404{suffix}）：请到音色库确认 voice_id 是否可用（{VOICE_LIBRARY_URL}）"
        ),
        400 => format!(
            "MOSS 拒绝了请求（400{suffix}）：接口严格校验入参，请检查音色 ID 与模型是否有效"
        ),
        429 => format!("MOSS 请求过于频繁（429{suffix}），请稍后重试"),
        _ => format!("MOSS 合成失败（HTTP {code}{suffix}）"),
    }
}

/// 取错误体里的服务端 message（官方结构 `{"error":{"message":…}}`，兼容顶层 `{"message":…}`）。
/// 只取这一句而不整体回显：原始错误体可能带着请求回声。
pub fn parse_error_message(body: &str) -> Option<String> {
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

    fn req() -> Request {
        Request {
            api_key: "K1".into(),
            model: DEFAULT_MODEL.into(),
            voice_id: "V1".into(),
        }
    }

    fn params_of(pairs: &[(&str, &str)]) -> AudioParams {
        parse_audio_params(pairs.iter().copied())
    }

    // ── 凭据与模型 ────────────────────────────────────────

    #[test]
    fn 缺key报中文错并给获取地址() {
        for key in [None, Some(""), Some("   ")] {
            let e = api_key_from(key.map(str::to_string)).unwrap_err();
            assert!(e.contains("API Key"), "实际: {e}");
            assert!(e.contains(API_KEY_URL), "应给出获取地址，实际: {e}");
        }
        assert_eq!(api_key_from(Some("  sk-1 ".to_string())).unwrap(), "sk-1");
    }

    #[test]
    fn 模型_空值用默认_有值原样() {
        assert_eq!(resolve_model(None), DEFAULT_MODEL);
        assert_eq!(resolve_model(Some("")), DEFAULT_MODEL);
        assert_eq!(resolve_model(Some("  ")), DEFAULT_MODEL);
        assert_eq!(resolve_model(Some(" moss-tts-1.0-pro ")), "moss-tts-1.0-pro");
        assert_eq!(DEFAULT_MODEL, "moss-tts-1.5-flash", "流式只认这个模型，别改回未文档化别名");
    }

    #[test]
    fn 缺音色报中文错并给音色库地址() {
        for voice in [None, Some(""), Some("   "), Some("default")] {
            let e = resolve_voice_id(voice).unwrap_err();
            assert!(e.contains("我的音色"), "应指向宿主的音色入口，实际: {e}");
            assert!(e.contains(VOICE_LIBRARY_URL), "应给出音色库地址，实际: {e}");
        }
        assert_eq!(resolve_voice_id(Some(" 11d3a27a-0951 ")).unwrap(), "11d3a27a-0951");
    }

    #[test]
    fn 空文本直接报错() {
        let e = resolve_request("   ", Some("V1"), Some("K1".into()), None).unwrap_err();
        assert!(e.contains("文本为空"), "实际: {e}");
    }

    #[test]
    fn 请求材料一次校验齐全() {
        let r = resolve_request("你好", Some(" V1 "), Some(" K1 ".into()), Some("moss-tts-1.0-pro")).unwrap();
        assert_eq!(
            r,
            Request {
                api_key: "K1".into(),
                model: "moss-tts-1.0-pro".into(),
                voice_id: "V1".into()
            }
        );
    }

    // ── 请求体 ────────────────────────────────────────────

    #[test]
    fn 非流式体只发文档规定的五个字段() {
        let body = build_request_body(&req(), "你好");
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["input"], "你好");
        assert_eq!(body["voice_id"], "V1");
        assert_eq!(body["response_format"], "mp3");
        assert_eq!(body["delivery_method"], "url");
        assert_eq!(body.as_object().unwrap().len(), 5, "接口严格校验，多余字段会 400：{body}");
    }

    #[test]
    fn 流式体带stream与pcm且不带禁用字段() {
        let body = build_stream_request_body(&req(), "你好");
        assert_eq!(body["stream"], true, "必须显式开流式");
        assert_eq!(body["response_format"], "pcm", "流式只能用 pcm");
        assert_eq!(body["stream_format"], "audio", "走裸 PCM 分块，不用 sse");
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["input"], "你好");
        assert_eq!(body["voice_id"], "V1");
        // 服务端对流式请求额外严格：这三个键一出现就 400 unsupported_for_stream
        for banned in ["delivery_method", "sample_rate", "file_id", "async", "webhook_url"] {
            assert!(body.get(banned).is_none(), "流式请求不得带 {banned}：{body}");
        }
        assert_eq!(body.as_object().unwrap().len(), 6, "字段数应恰好 6：{body}");
    }

    // ── 响应解析 ──────────────────────────────────────────

    #[test]
    fn 音频url抽取覆盖候选字段与嵌套() {
        // 文档示例确认的字段名是 url
        let doc = serde_json::json!({
            "id": "x", "object": "audio.speech", "status": "SUCCESS",
            "url": "https://cdn.example.com/out.mp3",
            "response_format": "mp3", "content_type": "audio/mpeg", "created_at": 1710000000
        });
        assert_eq!(extract_audio_url(&doc).unwrap(), "https://cdn.example.com/out.mp3");
        for f in URL_FIELDS {
            let resp = serde_json::json!({ f: "https://cdn.mosi.cn/a.mp3" });
            assert_eq!(extract_audio_url(&resp).unwrap(), "https://cdn.mosi.cn/a.mp3", "字段 {f}");
        }
        let nested = serde_json::json!({ "data": { "url": "https://cdn.mosi.cn/b.mp3" } });
        assert_eq!(extract_audio_url(&nested).unwrap(), "https://cdn.mosi.cn/b.mp3");
        // 非 http 值不算命中（可能是相对路径或 base64）
        assert!(extract_audio_url(&serde_json::json!({ "url": "/files/a.mp3" })).is_err());
    }

    #[test]
    fn 找不到url时报错只列字段名不回显内容() {
        let resp = serde_json::json!({ "data": { "detail": "用户不该被回显的合成文本" } });
        let e = extract_audio_url(&resp).unwrap_err();
        assert!(e.contains("音频 URL"), "实际: {e}");
        assert!(e.contains("data"), "要列出字段名帮助定位，实际: {e}");
        assert!(!e.contains("用户不该被回显的合成文本"), "错误串会被显示/落日志，不得回显响应内容：{e}");
    }

    #[test]
    fn 常见错误码映射成中文() {
        let e403 = friendly_error(403, r#"{"error":{"message":"permission denied"}}"#);
        assert!(e403.contains("API Key"), "网关缺 Key 常返 403，实际: {e403}");
        assert!(friendly_error(401, r#"{"error":{"message":"invalid key"}}"#).contains("API Key"));
        let e400 = friendly_error(400, r#"{"error":{"message":"invalid_request_error"}}"#);
        assert!(e400.contains("400") && e400.contains("invalid_request_error"), "实际: {e400}");
        // 文档具名错误码要给可操作建议
        let e404 = friendly_error(404, r#"{"error":{"code":"voice_not_found","message":"nope"}}"#);
        assert!(e404.contains(VOICE_LIBRARY_URL), "音色不存在要指向音色库，实际: {e404}");
        let e500 = friendly_error(500, "Internal Server Error");
        assert!(e500.contains("500"), "解析不出结构也不要空文案，实际: {e500}");
    }

    // ── 流式音频参数 ──────────────────────────────────────

    #[test]
    fn 音频参数_从内容类型解析() {
        let p = params_of(&[("content-type", "audio/L16;rate=24000;channels=1")]);
        assert_eq!(p, AudioParams { sample_rate: 24000, channels: 1, from_headers: true });
    }

    #[test]
    fn 音频参数_认多种头写法且大小写不敏感() {
        let p = params_of(&[("X-Sample-Rate", "44100"), ("X-CHANNELS", "2")]);
        assert_eq!(p, AudioParams { sample_rate: 44100, channels: 2, from_headers: true });
        let p2 = params_of(&[("content-type", "AUDIO/PCM; RATE=48000 ; CHANNELS=1")]);
        assert_eq!(p2.sample_rate, 48000);
        let p3 = params_of(&[("x-audio-sample-rate", "16000")]);
        assert_eq!(p3.sample_rate, 16000);
    }

    #[test]
    fn 音频参数_读不到就回落并标记来源() {
        let p = params_of(&[("content-type", "application/octet-stream")]);
        assert_eq!(
            p,
            AudioParams { sample_rate: FALLBACK_SAMPLE_RATE, channels: FALLBACK_CHANNELS, from_headers: false },
            "回落时必须标 from_headers=false，探针据此判断真值有没有拿到"
        );
        assert_eq!(params_of(&[]).from_headers, false);
    }

    #[test]
    fn 回落采样率钉住实测真值() {
        // 2026-10-15 实网探针（scripts/test-moss-stream.ps1）实测：48000 Hz / 单声道，
        // 且响应头里带得到（from_headers=true）。回落值只在网关哪天不带头时才生效，
        // 但那时它必须是对的——按"常见 TTS 默认"写 24000 会直接出花栗鼠音，比报错更难查。
        assert_eq!(FALLBACK_SAMPLE_RATE, 48_000);
        assert_eq!(FALLBACK_CHANNELS, 1);
    }

    #[test]
    fn 音频参数_非法值不采信() {
        // 采样率离谱/声道为 0 都会让宿主配错输出设备，宁可回落
        assert_eq!(params_of(&[("x-sample-rate", "0")]).sample_rate, FALLBACK_SAMPLE_RATE);
        assert_eq!(params_of(&[("x-sample-rate", "99999999")]).sample_rate, FALLBACK_SAMPLE_RATE);
        assert_eq!(params_of(&[("x-channels", "0")]).channels, FALLBACK_CHANNELS);
        assert_eq!(params_of(&[("x-channels", "64")]).channels, FALLBACK_CHANNELS);
        assert_eq!(params_of(&[("x-sample-rate", "abc")]).sample_rate, FALLBACK_SAMPLE_RATE);
    }

    #[test]
    fn info契约_pcm_s16le_带解析到的采样率与声道() {
        let info: serde_json::Value = serde_json::from_str(&stream_info_json(&AudioParams {
            sample_rate: 24000,
            channels: 1,
            from_headers: true,
        }))
        .unwrap();
        assert_eq!(info["format"], "pcm_s16le", "宿主流式管线只认这个格式名");
        assert_eq!(info["sample_rate"], 24000);
        assert_eq!(info["channels"], 1);
    }

    // ── 字节对齐 ──────────────────────────────────────────

    #[test]
    fn 对齐_奇数尾巴跨块拼接() {
        let mut carry = None;
        let first = align_bytes(&mut carry, &[1, 2, 3]);
        assert_eq!(first, vec![1, 2]);
        assert_eq!(carry, Some(3));
        let second = align_bytes(&mut carry, &[4, 5, 6]);
        assert_eq!(second, vec![3, 4, 5, 6], "上一块的尾巴必须拼在这一块前面");
        assert_eq!(carry, None, "3+3=6 字节刚好配平，不该再留尾巴");
    }

    #[test]
    fn 对齐_偶数块不留尾巴() {
        let mut carry = None;
        assert_eq!(align_bytes(&mut carry, &[1, 2, 3, 4]), vec![1, 2, 3, 4]);
        assert_eq!(carry, None);
        assert_eq!(align_bytes(&mut carry, &[]), Vec::<u8>::new(), "空块不应凭空产出字节");
    }
}
