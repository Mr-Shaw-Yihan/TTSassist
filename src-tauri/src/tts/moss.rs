// Moss-TTS 引擎实现（Mossland API）
//
// 文档依据：doc/tts-api/moss-tts.md
// - POST https://api.mosi.cn/v1/audio/speech
// - Authorization: Bearer <key>
// - body: { model: "moss-tts", input, voice_id, response_format: "mp3", delivery_method: "url" }
// - delivery_method=url 同步返回含音频 URL 的 JSON

use std::path::PathBuf;
use crate::storage::types::gen_id;
use crate::tts::traits::TtsError;

pub struct MossEngine {
    api_key: String,
    voice_id: String,
    data_dir: PathBuf,
}

impl MossEngine {
    pub fn new(api_key: String, voice_id: String, data_dir: PathBuf) -> Self {
        Self { api_key, voice_id, data_dir }
    }

    /// 构造请求体
    fn build_request_body(text: &str, voice_id: &str) -> serde_json::Value {
        serde_json::json!({
            "model": "moss-tts",
            "input": text,
            "voice_id": voice_id,
            "response_format": "mp3",
            "delivery_method": "url"
        })
    }

    /// 从响应 JSON 中提取音频 URL（文档未给示例响应体，尝试常见字段名）
    fn extract_audio_url(response: &serde_json::Value) -> std::result::Result<String, TtsError> {
        // 尝试常见字段名
        for field in &["audio_url", "url", "output_url", "audio", "result_url"] {
            if let Some(url) = response[field].as_str() {
                if url.starts_with("http") {
                    return Ok(url.to_string());
                }
            }
        }
        // 也检查嵌套结构（如 data.url）
        if let Some(url) = response["data"]["url"].as_str() {
            if url.starts_with("http") {
                return Ok(url.to_string());
            }
        }
        Err(TtsError::DecodeAudio(
            format!("moss 响应中未找到音频 URL。响应体: {}", response)
        ))
    }

    /// 生成语音：POST → 取 URL → 下载 → 存 mp3
    pub async fn generate(&self, text: &str) -> std::result::Result<String, TtsError> {
        if self.api_key.is_empty() {
            return Err(TtsError::NoApiKey);
        }
        if self.voice_id.is_empty() {
            return Err(TtsError::Http {
                status: 400,
                body: "未配置 Moss-TTS voice_id，请在设置中填写".into(),
            });
        }

        // 1. POST 请求
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| TtsError::Network(format!("构建 HTTP 客户端失败: {e}")))?;

        let body = Self::build_request_body(text, &self.voice_id);
        let resp = client
            .post("https://api.mosi.cn/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("moss 请求失败: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TtsError::Http {
                status: status.as_u16(),
                body: body_text,
            });
        }

        // 2. 解析响应拿 URL
        let response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TtsError::DecodeAudio(format!("moss 响应 JSON 解析失败: {e}")))?;

        let audio_url = Self::extract_audio_url(&response)?;

        // 3. 下载音频
        let audio_bytes = client
            .get(&audio_url)
            .send()
            .await
            .map_err(|e| TtsError::Network(format!("下载 moss 音频失败: {e}")))?
            .bytes()
            .await
            .map_err(|e| TtsError::Network(format!("读取 moss 音频字节失败: {e}")))?;

        // 4. 存为 mp3
        let id = gen_id("m");
        let rel_path = format!("audio/{id}.mp3");
        let abs_path = self.data_dir.join(&rel_path);
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TtsError::WriteFile(format!("创建目录失败: {e}")))?;
        }
        std::fs::write(&abs_path, &audio_bytes)
            .map_err(|e| TtsError::WriteFile(format!("写入音频失败: {e}")))?;

        Ok(rel_path)
    }
}