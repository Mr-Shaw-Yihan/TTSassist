// MiMo ASR 插件：调用小米 MiMo-V2.5-ASR 云端语音识别。
//
// API 文档：https://mimo.mi.com/docs/zh-CN/quick-start/usage-guide/audio/Speech-Recognition
// 接口：OpenAI 兼容的 chat/completions，音频以 Base64 传入。
// 鉴权：api-key 请求头，从环境变量 MIMO_API_KEY 读取。
//
// 支持格式：wav / mp3（Base64 后 ≤ 10MB）
// 语言：auto / zh / en

use std::sync::OnceLock;
use base64::Engine;

plugin_api::va_asr_plugin! {
    id: "mimo-asr",
    name: "MiMo ASR（小米·云端）",
    version: "1.0.0",
    languages: r#"[{"code":"auto","label":"自动检测"},{"code":"zh","label":"中文"},{"code":"en","label":"English"}]"#,
    transcribe: transcribe,
}

const API_BASE: &str = "https://api.xiaomimimo.com/v1/chat/completions";
const MODEL: &str = "mimo-v2.5-asr";

/// 音频大小上限：MiMo API 要求 Base64 编码后 ≤ 10MB，
/// Base64 体积 ≈ 原始体积 × 4/3，故原始音频上限取 7MB（留余量）
const MAX_AUDIO_BYTES: usize = 7 * 1024 * 1024;

/// 全局 tokio 运行时（转写入口是同步 C ABI，异步 HTTP 在自己的运行时里 block_on）
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("MiMo ASR 插件创建运行时失败")
    })
}

/// 音频字节 + 可选语言 → 转写文本
fn transcribe(audio: &[u8], language: Option<&str>) -> Result<String, String> {
    if audio.is_empty() {
        return Err("音频为空，无法识别".to_string());
    }
    if audio.len() > MAX_AUDIO_BYTES {
        let mb = audio.len() as f64 / 1024.0 / 1024.0;
        return Err(format!(
            "音频过大（{mb:.1} MB），MiMo ASR 单次识别上限约 7 MB，请缩短录音时长"
        ));
    }

    let api_key = std::env::var("MIMO_API_KEY")
        .map_err(|_| "未配置 MIMO_API_KEY，请在设置中填入 MiMo API Key".to_string())?;

    if api_key.is_empty() {
        return Err("MIMO_API_KEY 为空，请先在设置中配置 MiMo API Key".to_string());
    }

    // 检测音频格式（WAV 以 RIFF 开头，MP3 以 0xFF 0xFB/0xF3/0xF2 或 ID3 开头）
    let mime_type = detect_audio_mime(audio);

    let audio_base64 = base64::engine::general_purpose::STANDARD.encode(audio);
    let lang = language.unwrap_or("auto");

    let audio = audio_base64;
    let key = api_key;
    let language = lang.to_string();

    runtime().block_on(async move {
        call_mimo_asr(&key, &audio, &mime_type, &language).await
    })
}

/// 检测音频 MIME 类型
fn detect_audio_mime(audio: &[u8]) -> String {
    if audio.len() >= 4 && &audio[0..4] == b"RIFF" {
        "audio/wav".to_string()
    } else if audio.len() >= 3 && &audio[0..3] == b"ID3" {
        "audio/mp3".to_string()
    } else if audio.len() >= 2 && (audio[0] == 0xFF && (audio[1] & 0xE0) == 0xE0) {
        "audio/mp3".to_string()
    } else {
        // 默认当 wav
        "audio/wav".to_string()
    }
}

/// 调用 MiMo ASR API（OpenAI 兼容 chat/completions）
async fn call_mimo_asr(
    api_key: &str,
    audio_base64: &str,
    mime_type: &str,
    language: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": MODEL,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": format!("data:{mime_type};base64,{audio_base64}")
                        }
                    }
                ]
            }
        ],
        "asr_options": {
            "language": language
        }
    });

    let resp = client
        .post(API_BASE)
        .header("api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("MiMo ASR 请求失败: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // 常见错误友好提示
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(format!("MiMo ASR 鉴权失败（{status}）：API Key 无效或已过期，请在设置中检查"));
        }
        if status.as_u16() == 429 {
            return Err("MiMo ASR 请求过于频繁（429）：已触发限流，请稍后重试".to_string());
        }
        return Err(format!("MiMo ASR 返回 {status}: {text}"));
    }

    // 解析 OpenAI 兼容响应：choices[0].message.content
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("MiMo ASR 响应解析失败: {e}"))?;

    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        // 可能是空音频或无法识别
        return Ok(String::new());
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_检测正确() {
        let wav = b"RIFF\x00\x00\x00\x00WAVE";
        assert_eq!(detect_audio_mime(wav), "audio/wav");
    }

    #[test]
    fn mp3_id3_检测正确() {
        let mp3 = b"ID3\x04\x00";
        assert_eq!(detect_audio_mime(mp3), "audio/mp3");
    }

    #[test]
    fn mp3_frame_检测正确() {
        let mp3 = [0xFF, 0xFB, 0x90, 0x00];
        assert_eq!(detect_audio_mime(&mp3), "audio/mp3");
    }

    #[test]
    fn 未知格式默认wav() {
        let data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(detect_audio_mime(&data), "audio/wav");
    }

    #[test]
    fn 空音频拒绝() {
        let err = transcribe(&[], None).unwrap_err();
        assert!(err.contains("音频为空"), "错误文案不对: {err}");
    }

    #[test]
    fn 超大音频拒绝() {
        let big = vec![0u8; MAX_AUDIO_BYTES + 1];
        let err = transcribe(&big, None).unwrap_err();
        assert!(err.contains("音频过大"), "错误文案不对: {err}");
    }

    /// 集成测试：需要环境变量 MIMO_API_KEY 为有效的 API Key。
    /// 默认跳过（cargo test 不跑），手动用 `cargo test -- --ignored` 运行。
    /// 测试音频：项目根目录的 曼波.mp3
    #[test]
    #[ignore]
    fn 真实MiMo_ASR连通性测试() {
        let audio_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../曼波.mp3");
        let audio = std::fs::read(&audio_path)
            .unwrap_or_else(|e| panic!("读取测试音频失败: {} ({e})", audio_path.display()));

        let result = transcribe(&audio, Some("zh"));
        println!("转写结果: {:?}", result);
        assert!(result.is_ok(), "转写失败: {:?}", result.err());
        let text = result.unwrap();
        assert!(!text.is_empty(), "转写结果为空");
        println!("识别文本: {text}");
    }
}
