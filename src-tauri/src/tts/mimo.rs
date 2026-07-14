// MiMoTTS v2.5 引擎实现。
//
// 基于核实后的 MiMo 文档（doc/mimo-tts-v2.5.md）：
// - HTTP POST → https://api.xiaomimimo.com/v1/chat/completions（OpenAI 兼容 endpoint）
// - 请求头 api-key（非 Authorization Bearer）
// - 文本放 role:"assistant" 的 content
// - 非流式返回 choices[0].message.audio.data（base64 编码的 wav 字节）

use std::path::PathBuf;
use super::traits::{TTSEngine, TTSParams, TtsError, EngineCategory};
use crate::storage::types::{gen_id, now_iso};

/// 默认预置音色 id
const DEFAULT_VOICE: &str = "mimo_default";

/// MiMo TTS v2.5 引擎
pub struct MimoEngine {
    api_key: String,
    base_url: String,
    model: String,
    data_dir: PathBuf,
}

impl MimoEngine {
    /// 创建 MiMo 引擎实例。
    ///
    /// `api_key` 从 settings.mimo_api_key 传入（空字符串表示未配置）。
    /// `data_dir` 是 app_data_dir，所有音频文件都放其 audio/ 子目录。
    pub fn new(api_key: String, data_dir: PathBuf) -> Self {
        Self {
            api_key,
            base_url: "https://api.xiaomimimo.com/v1/chat/completions".into(),
            model: "mimo-v2.5-tts".into(),
            data_dir,
        }
    }

    /// 构造请求体 JSON（抽象为 pub(crate) 方便测试）。
    ///
    /// `instruction` 为风格指令（user 消息内容），None 或空时留空。
    fn build_request_body(
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

    /// 从响应 JSON 中提取音频字节（pub(crate) 方便测试）。
    fn extract_audio_data(response: &serde_json::Value) -> std::result::Result<Vec<u8>, TtsError> {
        use base64::Engine as _;
        let data_str = response["choices"][0]["message"]["audio"]["data"]
            .as_str()
            .ok_or_else(|| TtsError::DecodeAudio("响应中缺少 choices[0].message.audio.data".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(data_str)
            .map_err(|e| TtsError::DecodeAudio(format!("base64 解码失败: {e}")))
    }
}

#[async_trait::async_trait]
impl TTSEngine for MimoEngine {
    fn name(&self) -> &str {
        "mimo"
    }

    fn category(&self) -> EngineCategory {
        EngineCategory::Remote
    }

    async fn generate(&self, params: TTSParams<'_>) -> std::result::Result<String, TtsError> {
        // 1. 检查 key
        if self.api_key.is_empty() {
            return Err(TtsError::NoApiKey);
        }

        // 2. 生成消息 id 和时间
        let id = gen_id("m");
        let _created = now_iso();

        // 3. 构造请求体
        let body = Self::build_request_body(&self.model, params.text, params.voice, params.instruction);

        // 4. 发起 HTTP 请求
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| TtsError::Network(format!("构建 HTTP 客户端失败: {e}")))?;

        let resp = client
            .post(&self.base_url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("请求失败: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TtsError::Http {
                status: status.as_u16(),
                body: body_text,
            });
        }

        // 5. 解析响应
        let response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TtsError::DecodeAudio(format!("响应 JSON 解析失败: {e}")))?;

        let wav_bytes = Self::extract_audio_data(&response)?;

        // 6. 写文件
        let rel_path = format!("audio/{id}.wav");
        let abs_path = self.data_dir.join(&rel_path);
        // 确保 audio/ 目录存在
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TtsError::WriteFile(format!("创建目录失败: {e}")))?;
        }
        std::fs::write(&abs_path, &wav_bytes)
            .map_err(|e| TtsError::WriteFile(format!("写入文件失败: {e}")))?;

        // 7. 返回相对路径
        Ok(rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn build请求体_文本放assistant() {
        let body = MimoEngine::build_request_body("mimo-v2.5-tts", "你好世界", Some("冰糖"), None);
        assert_eq!(body["model"], "mimo-v2.5-tts");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][1]["content"], "你好世界");
        assert_eq!(body["audio"]["format"], "wav");
        assert_eq!(body["audio"]["voice"], "冰糖");
    }

    #[test]
    fn build请求体_默认音色() {
        let body = MimoEngine::build_request_body("mimo-v2.5-tts", "x", None, None);
        assert_eq!(body["audio"]["voice"], DEFAULT_VOICE);
    }

    #[test]
    fn build请求体_带风格指令() {
        let body = MimoEngine::build_request_body(
            "mimo-v2.5-tts",
            "你好",
            None,
            Some("请以约 1.3 倍速度朗读"),
        );
        assert_eq!(body["messages"][0]["content"], "请以约 1.3 倍速度朗读");
        assert_eq!(body["messages"][1]["content"], "你好");
    }

    #[test]
    fn 提取音频字节_正常() {
        // 造一个假 wav 的 base64
        let fake_wav = b"RIFFxxxxWAVEfake_wav_data";
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(fake_wav);

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "audio": {
                        "data": b64
                    }
                }
            }]
        });

        let result = MimoEngine::extract_audio_data(&response).unwrap();
        assert_eq!(result.as_slice(), fake_wav, "base64 解码后应与原始字节一致");
    }

    #[test]
    fn 提取音频字节_缺字段返回错误() {
        let response = serde_json::json!({ "choices": [] });
        let err = MimoEngine::extract_audio_data(&response).unwrap_err();
        assert!(matches!(err, TtsError::DecodeAudio(_)));
    }

    #[allow(non_snake_case)]
    #[test]
    fn 空key返回NoApiKey() {
        let dir = tempdir().unwrap();
        let engine = MimoEngine::new(String::new(), dir.path().to_path_buf());
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(engine.generate(TTSParams::new("你好")));
        assert!(matches!(result, Err(TtsError::NoApiKey)));
    }

    #[test]
    fn 引擎名和类别正确() {
        let dir = tempdir().unwrap();
        let engine = MimoEngine::new("test_key".into(), dir.path().to_path_buf());
        assert_eq!(engine.name(), "mimo");
        assert!(matches!(engine.category(), EngineCategory::Remote));
    }

    /// 集成测试：需要环境变量 MIMO_API_KEY 为有效的 API Key。
    /// 默认跳过（cargo test 不跑），手动用 `cargo test -- --ignored` 运行。
    #[allow(non_snake_case)]
    #[tokio::test]
    #[ignore]
    async fn 真实MiMo连通性测试() {
        let api_key = std::env::var("MIMO_API_KEY").expect("需要 MIMO_API_KEY 环境变量");
        let dir = tempdir().unwrap();
        let engine = MimoEngine::new(api_key, dir.path().to_path_buf());
        let path = engine
            .generate(TTSParams::new("你好，这是一段测试语音。"))
            .await
            .expect("MiMo 应成功返回音频");
        println!("生成的音频相对路径: {path}");
        let abs = dir.path().join(&path);
        assert!(abs.exists(), "音频文件应存在于磁盘");
        let size = std::fs::metadata(&abs).unwrap().len();
        assert!(size > 100, "wav 文件应大于 100 字节（至少有文件头）");
        println!("文件大小: {size} 字节");
    }
}