// MiMo TTS 协议层：请求体 / 响应抽取 / SSE 行解析 / WAV 容器剥离 / 克隆样本 URI。
//
// 全部为不碰网络的纯函数（克隆 URI 与模型解析只读环境变量），便于无网络单测。
// 协议事实（核实自 MiMo 官方文档，见 doc/MiMo插件化与流式默认设计.md §3.1）：
// - POST https://api.xiaomimimo.com/v1/chat/completions（OpenAI 兼容路径）；
// - 鉴权头是 api-key（不是 Authorization: Bearer）；
// - 待合成文本放 role:"assistant" 的 content（不是 user）；
// - 非流式返回 choices[0].message.audio.data = base64 WAV；
// - 流式（stream:true + audio.format=pcm16）走 SSE，每行 data: 里
//   choices[0].delta.audio.data 是 base64 裸 PCM（24kHz/16bit/mono，无 WAV 头）。

use base64::Engine as _;
use std::path::Path;

/// MiMo TTS 端点
pub const ENDPOINT: &str = "https://api.xiaomimimo.com/v1/chat/completions";
/// 预置音色模型（可经 MIMO_TTS_MODEL 覆盖，供官方上新档位不改代码）
pub const DEFAULT_MODEL: &str = "mimo-v2.5-tts";
/// 克隆音色模型（voice == "clone" 时强制切换）
pub const CLONE_MODEL: &str = "mimo-v2.5-tts-voiceclone";
/// 未指定音色时的服务端默认音色
pub const DEFAULT_VOICE: &str = "mimo_default";
/// 官方口径：format=pcm16 即 24kHz PCM16LE 单声道裸 PCM
pub const STREAM_SAMPLE_RATE: u32 = 24_000;
/// 流式声道数
pub const STREAM_CHANNELS: u32 = 1;
/// 克隆样本上限（官方 10MB）。宿主只做通用防呆，精确上限在此
pub const CLONE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 非流式请求体
pub fn build_request_body(
    model: &str,
    text: &str,
    voice: Option<&str>,
    instruction: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "user", "content": instruction.unwrap_or("") },
            { "role": "assistant", "content": text }
        ],
        "audio": {
            "format": "wav",
            "voice": voice.unwrap_or(DEFAULT_VOICE)
        }
    })
}

/// 流式请求体：在非流式请求体上只改两处（复用以免两条路径参数漂移）
pub fn build_stream_request_body(
    model: &str,
    text: &str,
    voice: Option<&str>,
    instruction: Option<&str>,
) -> serde_json::Value {
    let mut body = build_request_body(model, text, voice, instruction);
    body["audio"]["format"] = serde_json::Value::String("pcm16".to_string());
    body["stream"] = serde_json::Value::Bool(true);
    body
}

/// 非流式响应 → wav 字节
pub fn extract_audio_data(response: &serde_json::Value) -> Result<Vec<u8>, String> {
    let data_str = response["choices"][0]["message"]["audio"]["data"]
        .as_str()
        .ok_or_else(|| "MiMo 响应格式异常：缺少 choices[0].message.audio.data".to_string())?;
    base64::engine::general_purpose::STANDARD
        .decode(data_str)
        .map_err(|e| format!("MiMo 响应音频 base64 解码失败: {e}"))
}

/// 一行 SSE → 裸 PCM 字节。
///
/// `None` = 本行无音频（空行 / `:` 注释 / `event:` 头 / `[DONE]` / 该 chunk 无音频字段）；
/// `Some(Err)` = 格式错误，调用方应中止本次流。
pub fn sse_line_to_pcm(line: &str) -> Option<Result<Vec<u8>, String>> {
    let payload = match line.strip_prefix("data:") {
        Some(p) => p.trim(),
        None => return None,
    };
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => return Some(Err(format!("流式 chunk JSON 解析失败: {e}"))),
    };
    // Value 索引对 Null / 越界安全：choices 缺失或空数组时拿到 Null，进而无 audio 字段
    let b64 = value["choices"][0]["delta"]["audio"]["data"].as_str()?;
    Some(
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("流式音频 base64 解码失败: {e}")),
    )
}

/// 若字节是完整 WAV（RIFF…WAVE），返回其 `data` 子块的裸 PCM；不是 WAV 返回 `None`。
///
/// 为什么要有这一步：官方称 voiceclone 的低延迟流式未上线时服务端会**自动退化为兼容模式**
/// （推理完成后一次性返回）。存档分支实测拿到过 1 块且能出声，但没断言过那 1 块是裸 PCM
/// 还是 WAV 容器。若是后者，宿主按 `pcm_s16le` 喂给 rodio → 开头 44 字节变爆音，
/// 且归档 WAV 会双头。这里做一次防御性剥离（真流式的块不会命中）。
pub fn strip_wav_container(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let cc = &bytes[off..off + 4];
        let size = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().ok()?) as usize;
        let body = off + 8;
        if cc == b"data" {
            let end = body.checked_add(size).map(|e| e.min(bytes.len())).unwrap_or(bytes.len());
            return Some(bytes[body..end].to_vec());
        }
        // WAV 子块按偶数字节对齐（奇数长度补一字节填充）
        off = body + size + (size & 1);
    }
    None
}

/// 把一块 PCM 裁到 2 字节对齐，落单字节暂存到 `carry` 供下一块拼接
pub fn align_bytes(carry: &mut Option<u8>, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if let Some(b) = carry.take() {
        out.push(b);
    }
    out.extend_from_slice(bytes);
    if out.len() % 2 == 1 {
        *carry = out.pop();
    }
    out
}

/// 按扩展名声明 MIME（MiMo 支持 mp3/wav/m4a/flac/ogg；MIME 与真实格式不符会被
/// 服务端判 "invalid audio format"，故这张表必须与官方一致）
pub fn mime_for_ext(ext: &str) -> Result<&'static str, String> {
    Ok(match ext.to_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        other => {
            return Err(format!(
                "不支持的克隆样本格式「{other}」，请改用 mp3 / wav / m4a / flac / ogg"
            ))
        }
    })
}

/// 解析模型配置值（纯函数：环境变量读取留在 `configured_model` 薄壳里，
/// 避开并发测试交叉读写进程全局 env）
pub fn configured_model_from(raw: Option<String>) -> String {
    raw.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// 读合成模型（空值回退默认）。env 名与 mimo-asr 的 MIMO_API_KEY 区分开，
/// 避免宿主必填 env 冲突检测拒载
pub fn configured_model() -> String {
    configured_model_from(std::env::var("MIMO_TTS_MODEL").ok())
}

/// 读 API Key（由宿主通用配置机制按 manifest.config 注入）
pub fn read_api_key() -> Result<String, String> {
    std::env::var("MIMO_TTS_API_KEY")
        .map(|k| k.trim().to_string())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "未配置 MiMo API Key：请在「语音中心 → MiMo TTS」的插件设置中填写".to_string())
}

/// 把音色参数拆成 (模型, 请求里的 voice)。
///
/// 环境变量读取由 `resolve_voice` 薄壳完成后递进来，本函数为纯函数（可测且不跳线程共享状态）：
/// - `voice == Some("clone")` → `mimo-v2.5-tts-voiceclone` + 样本 data URI
///   （样本路径来自宿主注入的通用环境变量 `VA_CLONE_SAMPLE`）
/// - 其余（`None` / 空串 / 预置音色 id）→ 配置的基模型 + 原样透传，空串回落默认音色
pub fn resolve_voice_from(
    model: &str,
    voice: Option<&str>,
    sample_raw: Option<String>,
) -> Result<(String, Option<String>), String> {
    if voice == Some("clone") {
        return Ok((
            CLONE_MODEL.to_string(),
            Some(build_clone_voice_uri_from(sample_raw)?),
        ));
    }
    let effective = voice.map(str::trim).filter(|v| !v.is_empty()).unwrap_or(DEFAULT_VOICE);
    Ok((model.to_string(), Some(effective.to_string())))
}

/// 环境变量薄壳：取 `MIMO_TTS_MODEL` / `VA_CLONE_SAMPLE` 后交给纯函数
pub fn resolve_voice(voice: Option<&str>) -> Result<(String, Option<String>), String> {
    resolve_voice_from(
        &configured_model(),
        voice,
        std::env::var("VA_CLONE_SAMPLE").ok(),
    )
}

/// 克隆样本 → data URI（纯函数：`sample_raw` 来自 `VA_CLONE_SAMPLE`，`None` = 未导入）
///
/// 每次合成都要把样本传给服务端，故读一次算一次。
pub fn build_clone_voice_uri_from(sample_raw: Option<String>) -> Result<String, String> {
    let no_sample = "未导入克隆音色样本：请在「语音中心 → 音色克隆」导入一段 5–10 秒的清晰人声（mp3/wav/m4a/flac/ogg）";
    let raw = sample_raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let Some(raw) = raw else { return Err(no_sample.to_string()) };
    let path = Path::new(&raw);
    // 先看尺寸再读文件：避免用户把 env 指到大文件时先全量吃进内存才报错
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("克隆样本文件读不到（{}）：{e}", path.display()))?;
    let len = meta.len();
    if len > CLONE_MAX_BYTES {
        return Err(format!(
            "克隆样本 {:.1}MB 超出 MiMo 上限 10MB，请换更短的样本",
            len as f64 / 1024.0 / 1024.0
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("克隆样本文件读取失败（{}）：{e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mime = mime_for_ext(ext)?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// 流式元信息 JSON（宿主据此装配播放器；当前契约只认 pcm_s16le）
pub fn stream_info_json() -> String {
    format!(
        r#"{{"format":"pcm_s16le","sample_rate":{},"channels":{}}}"#,
        STREAM_SAMPLE_RATE, STREAM_CHANNELS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一行 SSE：data: {...delta.audio.data: base64...}
    fn data_line(bytes: &[u8]) -> String {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "audio": { "data": b64 } } }] })
        )
    }

    #[test]
    fn 请求体_文本放assistant() {
        let body = build_request_body("mimo-v2.5-tts", "你好世界", Some("冰糖"), None);
        assert_eq!(body["model"], "mimo-v2.5-tts");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][1]["content"], "你好世界");
        assert_eq!(body["audio"]["voice"], "冰糖");
        assert_eq!(body["audio"]["format"], "wav");
    }

    #[test]
    fn 请求体_缺省音色与指令() {
        let body = build_request_body("mimo-v2.5-tts", "x", None, None);
        assert_eq!(body["audio"]["voice"], DEFAULT_VOICE);
        assert_eq!(body["messages"][0]["content"], "");
    }

    #[test]
    fn 流式请求体_只改两处() {
        let plain = build_request_body("mimo-v2.5-tts", "你好", Some("冰糖"), None);
        let stream = build_stream_request_body("mimo-v2.5-tts", "你好", Some("冰糖"), None);
        assert_eq!(stream["stream"], true);
        assert_eq!(stream["audio"]["format"], "pcm16");
        // 其余字段必须与非流式一致（防两条路径参数漂移）
        assert_eq!(stream["messages"], plain["messages"]);
        assert_eq!(stream["model"], plain["model"]);
    }

    #[test]
    fn 非流式响应_取base64wav() {
        use base64::Engine as _;
        let fake = b"RIFFxxxxWAVEfake_wav_data";
        let resp = serde_json::json!({
            "choices": [{ "message": { "audio": { "data": base64::engine::general_purpose::STANDARD.encode(fake) } } }]
        });
        assert_eq!(extract_audio_data(&resp).unwrap(), fake.to_vec());
    }

    #[test]
    fn 非流式响应_缺字段给中文错() {
        let err = extract_audio_data(&serde_json::json!({ "choices": [] })).unwrap_err();
        assert!(err.contains("响应"), "实际: {err}");
    }

    #[test]
    fn sse_完整单行取出裸pcm() {
        let line = data_line(&[1, 2, 3, 4]);
        assert_eq!(sse_line_to_pcm(&line).unwrap().unwrap(), vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn sse_非音频行一律当无音频() {
        // 心跳空行 / 注释 / event 头 / [DONE] / data 后空体：不得报错，否则整条流会被误杀
        for line in ["", ": ping", "event: message", "data:", "data: [DONE]"] {
            assert!(sse_line_to_pcm(line).is_none(), "行「{line}」应视为无音频");
        }
    }

    #[test]
    fn sse_无音频字段的chunk跳过() {
        // 对齐官方 Python 示例 if not chunk.choices: continue（纯文本 chunk / 末尾 usage）
        for chunk in [
            serde_json::json!({ "choices": [] }),
            serde_json::json!({ "choices": [{ "delta": { "content": "你好" } }] }),
            serde_json::json!({ "choices": [{ "delta": { "audio": null } }] }),
            serde_json::json!({ "usage": { "total_tokens": 1 } }),
        ] {
            assert!(sse_line_to_pcm(&format!("data: {chunk}")).is_none());
        }
    }

    #[test]
    fn sse_格式错误才报错() {
        assert!(matches!(sse_line_to_pcm("data: {not json"), Some(Err(_))));
        let bad_b64 = format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "audio": { "data": "!!!" } } }] })
        );
        let err = sse_line_to_pcm(&bad_b64).unwrap().unwrap_err();
        assert!(err.contains("base64"), "实际: {err}");
    }

    #[test]
    fn wav容器_识别并剥出data子块() {
        // 兼容模式若把音频整体塞进 WAV，宿主契约（pcm_s16le）会被 44 字节头污染
        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[12..16].copy_from_slice(b"fmt ");
        wav[16..20].copy_from_slice(&16u32.to_le_bytes());
        wav[36..40].copy_from_slice(b"data");
        wav[40..44].copy_from_slice(&4u32.to_le_bytes());
        let pcm = [1u8, 2, 3, 4];
        wav.extend_from_slice(&pcm);
        assert_eq!(strip_wav_container(&wav).unwrap(), pcm.to_vec());
    }

    #[test]
    fn wav容器_裸pcm原样返回不误剔() {
        // 24kHz 音频块开头恰好凑出 "RIFF" 的概率极低，但仍要求：非 WAV 一律不动
        assert!(strip_wav_container(&[0x00, 0x01, 0x02, 0x03]).is_none());
        let mut fake = vec![0u8; 20];
        fake[0..4].copy_from_slice(b"RIFF");
        assert!(strip_wav_container(&fake).is_none(), "太短不足以是 WAV");
    }

    #[test]
    fn wav容器_尺寸字段超出实际长度时按实际截断() {
        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[12..16].copy_from_slice(b"fmt ");
        wav[36..40].copy_from_slice(b"data");
        wav[40..44].copy_from_slice(&9999u32.to_le_bytes()); // 谎报长度
        wav.extend_from_slice(&[9u8; 6]);
        assert_eq!(strip_wav_container(&wav).unwrap().len(), 6);
    }

    #[test]
    fn 字节对齐_奇数尾巴进下一块() {
        // 宿主按 chunks_exact(2) 转采样，落单字节会被静默丢弃 → 一旦丢就永久相位错开
        let mut carry: Option<u8> = None;
        let a = align_bytes(&mut carry, &[0x34, 0x12, 0x78]);
        assert_eq!(a, vec![0x34u8, 0x12]);
        assert_eq!(carry, Some(0x78));
        let b = align_bytes(&mut carry, &[0x56]);
        assert_eq!(b, vec![0x78u8, 0x56]);
        assert_eq!(carry, None);
    }

    #[test]
    fn mime_五种官方格式() {
        assert_eq!(mime_for_ext("mp3").unwrap(), "audio/mpeg");
        assert_eq!(mime_for_ext("WAV").unwrap(), "audio/wav");
        assert_eq!(mime_for_ext("m4a").unwrap(), "audio/mp4");
        assert_eq!(mime_for_ext("flac").unwrap(), "audio/flac");
        assert_eq!(mime_for_ext("ogg").unwrap(), "audio/ogg");
        assert!(mime_for_ext("aac").is_err());
    }

    #[test]
    fn 音色解析_预置与克隆() {
        // 哨兵模型：必须与 DEFAULT_MODEL 不同，才能证明模型真从配置读入而非硬编码
        const OVERRIDE: &str = "mimo-v9.9-tts-sentinel";
        // 预置：voice 原样透传
        let (model, voice) = resolve_voice_from(OVERRIDE, Some("冰糖"), None).unwrap();
        assert_eq!(model, OVERRIDE);
        assert_eq!(voice.as_deref(), Some("冰糖"));
        // 缺省与空串都回落服务端默认音色（空串透传给服务端会被判 invalid voice）
        let (_, voice) = resolve_voice_from(OVERRIDE, None, None).unwrap();
        assert_eq!(voice.as_deref(), Some(DEFAULT_VOICE));
        let (_, voice) = resolve_voice_from(OVERRIDE, Some("  "), None).unwrap();
        assert_eq!(voice.as_deref(), Some(DEFAULT_VOICE));
        // 克隆：强制切 voiceclone 模型；无样本→ 中文错并含导入指引
        let err = resolve_voice_from(OVERRIDE, Some("clone"), None).unwrap_err();
        assert!(err.contains("克隆"), "实际: {err}");
        // 样本路径指个不存在的文件：要报“读不到”而不是误导向“未导入”
        let err = resolve_voice_from(OVERRIDE, Some("clone"), Some("C:/va-no-such/clone.mp3".into()))
            .unwrap_err();
        assert!(err.contains("克隆样本"), "实际: {err}");
        assert_eq!(CLONE_MODEL, "mimo-v2.5-tts-voiceclone");
    }

    #[test]
    fn 模型缺省值_空值回落不读env() {
        // 只测纯函数：环境变量读写是进程全局的，并发测试跳着 set/remove
        // 同一变量在 Windows CRT 下不是线程安全的，不能拿它做单测对象
        assert_eq!(configured_model_from(None), DEFAULT_MODEL);
        assert_eq!(configured_model_from(Some(String::new())), DEFAULT_MODEL);
        assert_eq!(configured_model_from(Some("   ".into())), DEFAULT_MODEL);
        assert_eq!(
            configured_model_from(Some("mimo-v9.9-tts-sentinel".into())),
            "mimo-v9.9-tts-sentinel",
            "上新档位靠配置覆盖，不该回落到硬编码默认值"
        );
        // 前后空白要清掉（宿主注入值可能带空格）
        assert_eq!(configured_model_from(Some(" mimo-v2.5-tts ".into())), "mimo-v2.5-tts");
    }
}
