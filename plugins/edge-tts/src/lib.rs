// Edge-TTS 插件：白嫖微软 Edge"大声朗读"服务，免费、无需 API Key。
//
// 用 kothok-edge-tts 库（处理 Sec-MS-GEC 验证 token，避免 403），输出 MP3。
// ⚠️ 非官方接口，微软改协议可能失效；部分地区可能 403。定位"免费备选"。
//
// 本 crate 由主程序的插件加载框架（libloading）加载，逻辑从主程序 tts/edge.rs 抽出。

use std::sync::{Once, OnceLock};

plugin_api::va_tts_plugin! {
    id: "edge-tts",
    name: "Edge TTS（免费·微软）",
    version: "1.0.0",
    audio_format: "mp3",
    voices_json: r#"[{"id":"zh-CN-XiaoxiaoNeural","label":"晓晓（女·温暖）"},{"id":"zh-CN-YunxiNeural","label":"云希（男·青年）"},{"id":"zh-CN-YunyangNeural","label":"云扬（男·新闻）"},{"id":"zh-CN-XiaoyiNeural","label":"晓伊（女·活泼）"},{"id":"zh-CN-YunjianNeural","label":"云健（男·体育）"},{"id":"zh-CN-XiaochenNeural","label":"晓辰（女·知性）"}]"#,
    synthesize: synthesize,
}

/// 默认音色（晓晓，温暖女声）
pub const DEFAULT_VOICE: &str = "zh-CN-XiaoxiaoNeural";

/// 全局 tokio 运行时（合成入口是同步 C ABI，异步合成在自己的运行时里 block_on）
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Edge TTS 插件创建运行时失败")
    })
}

/// 从音色短名推导 BCP-47 语言标签：zh-CN-XiaoxiaoNeural → zh-CN
fn lang_from_voice(voice: &str) -> String {
    voice.split('-').take(2).collect::<Vec<_>>().join("-")
}

/// 文本 → MP3 字节（voice 为 None 用默认晓晓）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let voice = voice.unwrap_or(DEFAULT_VOICE).to_string();
    let text = text.to_string();

    runtime().block_on(async move {
        use kothok_edge_tts::{init_tls, EdgeTts, Engine, TtsEvent};

        // TLS 只需初始化一次（幂等，但用 Once 更干净）
        static INIT_TLS: Once = Once::new();
        INIT_TLS.call_once(|| init_tls());

        let lang = lang_from_voice(&voice);

        // 合成（流式返回 MP3 分片）
        let events = EdgeTts
            .synthesize(&text, &voice, "+0%", &lang)
            .await
            .map_err(|e| format!("Edge TTS 合成失败: {e}"))?;

        // 拼接所有音频分片成完整 MP3
        let mut audio = Vec::new();
        for ev in events {
            if let TtsEvent::Audio(bytes) = ev {
                audio.extend_from_slice(&bytes);
            }
        }
        if audio.is_empty() {
            return Err("Edge TTS 未返回音频（可能地区受限或网络异常）".to_string());
        }
        Ok(audio)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_推导正确() {
        assert_eq!(lang_from_voice("zh-CN-XiaoxiaoNeural"), "zh-CN");
        assert_eq!(lang_from_voice("en-US-AriaNeural"), "en-US");
        assert_eq!(lang_from_voice("单段"), "单段");
    }

    #[test]
    fn 音色表与宏内json一致() {
        // 与 va_tts_plugin! 里的 voices_json 保持同步（新增音色两处都要改）
        let json = r#"[{"id":"zh-CN-XiaoxiaoNeural","label":"晓晓（女·温暖）"},{"id":"zh-CN-YunxiNeural","label":"云希（男·青年）"},{"id":"zh-CN-YunyangNeural","label":"云扬（男·新闻）"},{"id":"zh-CN-XiaoyiNeural","label":"晓伊（女·活泼）"},{"id":"zh-CN-YunjianNeural","label":"云健（男·体育）"},{"id":"zh-CN-XiaochenNeural","label":"晓辰（女·知性）"}]"#;
        let voices: Vec<plugin_api::VoiceItem> = serde_json::from_str(json).unwrap();
        assert_eq!(voices.len(), 6);
        assert_eq!(voices[0].id, DEFAULT_VOICE);
    }
}
