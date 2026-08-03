// Edge-TTS 引擎（免费备选）：白嫖微软 Edge"大声朗读"服务，免费、无需 API Key。
//
// 用 kothok-edge-tts 库（处理 Sec-MS-GEC 验证 token，避免 403）。
// 输出 MP3（audio-24khz-48kbitrate-mono-mp3）。
//
// ⚠️ 非官方接口，微软改协议可能失效；部分地区可能 403。定位"免费备选"。

use std::path::PathBuf;
use std::sync::Once;
use crate::storage::types::gen_id;
use crate::tts::traits::{TTSEngine, TTSParams, TtsError};

/// TLS 只需初始化一次（幂等，但用 Once 更干净）
static INIT_TLS: Once = Once::new();

/// Edge TTS 默认音色（晓晓，温暖女声）
pub const DEFAULT_EDGE_VOICE: &str = "zh-CN-XiaoxiaoNeural";

/// 内置常用中文音色清单（首版不做完整几百个列表）
pub const EDGE_ZH_VOICES: &[(&str, &str)] = &[
    ("zh-CN-XiaoxiaoNeural", "晓晓（女·温暖）"),
    ("zh-CN-YunxiNeural", "云希（男·青年）"),
    ("zh-CN-YunyangNeural", "云扬（男·新闻）"),
    ("zh-CN-XiaoyiNeural", "晓伊（女·活泼）"),
    ("zh-CN-YunjianNeural", "云健（男·体育）"),
    ("zh-CN-XiaochenNeural", "晓辰（女·知性）"),
];

pub struct EdgeTtsEngine {
    data_dir: PathBuf,
}

impl EdgeTtsEngine {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }
}

/// 从音色短名推导 BCP-47 语言标签：zh-CN-XiaoxiaoNeural → zh-CN
fn lang_from_voice(voice: &str) -> String {
    voice.split('-').take(2).collect::<Vec<_>>().join("-")
}

#[async_trait::async_trait]
impl TTSEngine for EdgeTtsEngine {
    fn name(&self) -> &str {
        "edge"
    }

    fn category(&self) -> crate::tts::traits::EngineCategory {
        crate::tts::traits::EngineCategory::Remote
    }

    async fn generate(&self, params: TTSParams<'_>) -> Result<String, TtsError> {
        use kothok_edge_tts::{init_tls, EdgeTts, Engine, TtsEvent};

        INIT_TLS.call_once(|| init_tls());

        let voice = params.voice.unwrap_or(DEFAULT_EDGE_VOICE);
        let lang = lang_from_voice(voice);

        // 合成（流式返回 MP3 分片）
        let events = EdgeTts
            .synthesize(params.text, voice, "+0%", &lang)
            .await
            .map_err(|e| TtsError::Network(format!("Edge TTS 合成失败: {e}")))?;

        // 拼接所有音频分片成完整 MP3
        let mut audio = Vec::new();
        for ev in events {
            if let TtsEvent::Audio(bytes) = ev {
                audio.extend_from_slice(&bytes);
            }
        }
        if audio.is_empty() {
            return Err(TtsError::Network("Edge TTS 未返回音频（可能地区受限或网络异常）".into()));
        }

        // 保存为 MP3
        let id = gen_id("m");
        let rel_path = format!("audio/{id}.mp3");
        let abs = self.data_dir.join(&rel_path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TtsError::WriteFile(format!("创建目录失败: {e}")))?;
        }
        std::fs::write(&abs, &audio)
            .map_err(|e| TtsError::WriteFile(format!("写入音频失败: {e}")))?;

        Ok(rel_path)
    }
}
