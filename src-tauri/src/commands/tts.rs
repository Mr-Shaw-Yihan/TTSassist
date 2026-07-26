// generate_tts 命令：前端输入文本 → TTS 生成 → 写 messages.json → 广播。
// 支持多引擎分发：settings.tts_engine = "mimo" | "moss"

use std::path::Path;
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::{Message, gen_id, now_iso};
use crate::tts::mimo::MimoEngine;
use crate::tts::moss::MossEngine;
use crate::tts::traits::{TTSEngine, TTSParams, TtsError};
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

/// 把克隆样本音频转成 MiMo 要的 data URI（"data:audio/<ext>;base64,<b64>"）
fn build_clone_voice_uri(sample_path: &Path) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = std::fs::read(sample_path)
        .map_err(|e| format!("读取克隆样本失败: {e}"))?;
    let ext = sample_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "mp3".into());
    let mime = match ext.as_str() {
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// MiMo 引擎生成（含克隆音色）
async fn generate_mimo(
    text: &str,
    data_dir: &Path,
    api_key: &str,
    tts_model: &str,
    clone_voice_path: &str,
) -> Result<String, TtsError> {
    let is_clone = tts_model == "clone";

    let (engine, voice_string) = if is_clone {
        if clone_voice_path.is_empty() {
            return Err(TtsError::Http {
                status: 400,
                body: "未导入克隆音色样本，请在设置中导入".into(),
            });
        }
        let sample_abs = data_dir.join(clone_voice_path);
        if !sample_abs.exists() {
            return Err(TtsError::Http {
                status: 400,
                body: "克隆样本文件不存在，请重新导入".into(),
            });
        }
        let voice_uri = build_clone_voice_uri(&sample_abs)
            .map_err(|e| TtsError::Http { status: 400, body: e })?;
        let engine = MimoEngine::new(api_key.to_string(), data_dir.to_path_buf())
            .with_model("mimo-v2.5-tts-voiceclone");
        (engine, Some(voice_uri))
    } else {
        let voice = match tts_model {
            "" | "default" => None,
            v => Some(v.to_string()),
        };
        let engine = MimoEngine::new(api_key.to_string(), data_dir.to_path_buf());
        (engine, voice)
    };

    let params = TTSParams {
        text,
        voice: voice_string.as_deref(),
        instruction: None,
    };
    engine.generate(params).await
}

#[tauri::command]
pub async fn generate_tts(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Message, String> {
    let data_dir = state.data_dir.clone();

    // 1. 读设置
    let (tts_engine, mimo_key, moss_key, moss_voice, tts_model, clone_path) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        (
            s.tts_engine.clone(),
            s.mimo_api_key.clone(),
            s.moss_api_key.clone(),
            s.moss_voice_id.clone(),
            s.tts_model.clone(),
            s.clone_voice_path.clone(),
        )
    };

    // 2. 据引擎分发
    let audio_path = match tts_engine.as_str() {
        "moss" => {
            if moss_key.is_empty() {
                return Err("请在设置中填写 Moss-TTS API Key".into());
            }
            if moss_voice.is_empty() {
                return Err("请在设置中填写 Moss-TTS voice_id".into());
            }
            let engine = MossEngine::new(moss_key, moss_voice, data_dir.clone());
            engine.generate(&text).await.map_err(|e| format!("{e}"))?
        }
        _ => {
            // 默认走 mimo（含空字符串兜底）
            if mimo_key.is_empty() {
                return Err("请在设置中填写 MiMo API Key".into());
            }
            generate_mimo(&text, &data_dir, &mimo_key, &tts_model, &clone_path)
                .await
                .map_err(|e| format!("{e}"))?
        }
    };

    // 3. 保存消息记录
    let message = Message {
        id: gen_id("m"),
        content: text,
        audio_path,
        created_at: now_iso(),
    };
    let result = message.clone();
    crate::storage::messages::add_message(&data_dir, message)
        .map_err(|e| format!("保存消息失败: {e}"))?;

    // 4. 广播事件
    notify_changed(&app, EVENT_MESSAGE_CHANGED);

    Ok(result)
}