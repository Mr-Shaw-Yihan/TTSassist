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

/// 把克隆样本音频转成 MiMo 要的 data URI（"data:<mime>;base64,<b64>"）。
/// MiMo 克隆音色支持 mp3/flac/m4a/wav/ogg，须按实际格式声明正确的 MIME，
/// 否则 MiMo 会因 MIME 与真实格式不符而报 "invalid audio format"。
fn build_clone_voice_uri(sample_path: &Path) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = std::fs::read(sample_path)
        .map_err(|e| format!("读取克隆样本失败: {e}"))?;
    let ext = sample_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        other => {
            return Err(format!(
                "不支持的音频格式「{}」，克隆音色仅支持 mp3 / wav / m4a / flac / ogg",
                if other.is_empty() { "未知" } else { other }
            ));
        }
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
    mic: State<'_, crate::commands::mic::MicPlayback>,
) -> Result<Message, String> {
    let data_dir = state.data_dir.clone();

    // 1. 读设置
    let (tts_engine, mimo_key, moss_key, moss_voice, tts_model, clone_path, edge_voice,
         mic_send_enabled, mic_device, mic_volume) = {
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
            s.edge_voice.clone(),
            s.mic_send_enabled,
            s.mic_output_device.clone(),
            s.mic_playback_volume,
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
        "edge" => {
            // Edge TTS：免费、无需 key。音色用 edge_voice（空则用默认晓晓）
            let engine = crate::tts::edge::EdgeTtsEngine::new(data_dir.clone());
            let voice = if edge_voice.is_empty() { None } else { Some(edge_voice.as_str()) };
            let params = TTSParams {
                text: &text,
                voice,
                instruction: None,
            };
            engine.generate(params).await.map_err(|e| format!("{e}"))?
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

    // 5. 全局开关开启且配置了麦克风设备 → 同时发到虚拟麦克风（扬声器播放由前端负责）
    if mic_send_enabled && !mic_device.is_empty() {
        let abs = data_dir.join(&result.audio_path);
        mic.play(abs, mic_device, mic_volume);
    }

    Ok(result)
}

/// Edge TTS 内置中文音色清单（id + 显示名），供前端下拉选择。
#[tauri::command]
pub fn list_edge_voices() -> Vec<EdgeVoiceItem> {
    crate::tts::edge::EDGE_ZH_VOICES
        .iter()
        .map(|(id, label)| EdgeVoiceItem { id: (*id).to_string(), label: (*label).to_string() })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EdgeVoiceItem {
    pub id: String,
    pub label: String,
}