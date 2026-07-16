// generate_tts 命令：前端输入文本 → TTS 生成 → 写 messages.json → 广播。
// 支持预置音色（mimo-v2.5-tts）与克隆音色（mimo-v2.5-tts-voiceclone）。

use std::path::Path;
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::{Message, gen_id, now_iso};
use crate::tts::mimo::MimoEngine;
use crate::tts::traits::{TTSEngine, TTSParams};
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
    // MiMo 文档要求 mime 为 audio/mpeg 或 audio/wav
    let mime = match ext.as_str() {
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

#[tauri::command]
pub async fn generate_tts(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Message, String> {
    let data_dir = state.data_dir.clone();

    // 1. 读设置（放在代码块内，确保锁在 .await 前释放）
    let (api_key, tts_engine, tts_model, clone_voice_path) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;

        if s.mimo_api_key.is_empty() {
            return Err("请在设置中填写 MiMo API Key".into());
        }

        (
            s.mimo_api_key.clone(),
            s.tts_engine.clone(),
            s.tts_model.clone(),
            s.clone_voice_path.clone(),
        )
    }; // ← settings 读锁在此释放

    if tts_engine != "mimo" {
        return Err(format!("不支持的引擎: {tts_engine}，当前仅支持 mimo"));
    }

    // 2. 据 tts_model 决定预置 vs 克隆
    //    克隆约定：tts_model == "clone"
    let is_clone = tts_model == "clone";

    let (engine, voice_string) = if is_clone {
        if clone_voice_path.is_empty() {
            return Err("未导入克隆音色样本，请在设置中导入".into());
        }
        let sample_abs = data_dir.join(&clone_voice_path);
        if !sample_abs.exists() {
            return Err("克隆样本文件不存在，请重新导入".into());
        }
        let voice_uri = build_clone_voice_uri(&sample_abs)?;
        let engine = MimoEngine::new(api_key, data_dir.clone())
            .with_model("mimo-v2.5-tts-voiceclone");
        (engine, Some(voice_uri))
    } else {
        let voice = match tts_model.as_str() {
            "" | "default" => None,
            v => Some(v.to_string()),
        };
        let engine = MimoEngine::new(api_key, data_dir.clone());
        (engine, voice)
    };

    // 3. TTS 生成
    let params = TTSParams {
        text: &text,
        voice: voice_string.as_deref(),
        instruction: None,
    };
    let audio_path = engine
        .generate(params)
        .await
        .map_err(|e| format!("{e}"))?;

    // 4. 保存消息记录
    let message = Message {
        id: gen_id("m"),
        content: text,
        audio_path,
        created_at: now_iso(),
    };
    let result = message.clone();
    crate::storage::messages::add_message(&data_dir, message)
        .map_err(|e| format!("保存消息失败: {e}"))?;

    // 5. 广播事件
    notify_changed(&app, EVENT_MESSAGE_CHANGED);

    Ok(result)
}