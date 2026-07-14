// generate_tts 命令：前端输入文本 → TTS 生成 → 写 messages.json → 广播。

use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::{Message, gen_id, now_iso};
use crate::tts::mimo::MimoEngine;
use crate::tts::traits::{TTSEngine, TTSParams};
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

/// 文本转语音：生成音频 → 存消息记录 → 通知前端刷新。
///
/// 流程见开发记录.md 3.5 节。
#[tauri::command]
pub async fn generate_tts(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Message, String> {
    let data_dir = state.data_dir.clone();

    // 1. 读设置（放在代码块内，确保锁在 .await 前释放）
    let (api_key, tts_engine, voice_string, tts_speed) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;

        if s.mimo_api_key.is_empty() {
            return Err("请在设置中填写 MiMo API Key".into());
        }

        let voice = match s.tts_model.as_str() {
            "" | "default" => None,
            v => Some(v.to_string()),
        };

        (s.mimo_api_key.clone(), s.tts_engine.clone(), voice, s.tts_speed)
    }; // ← settings 读锁在此释放

    // 2. 构建引擎（首版只支持 mimo）
    if tts_engine != "mimo" {
        return Err(format!("不支持的引擎: {tts_engine}，当前仅支持 mimo"));
    }
    let engine = MimoEngine::new(api_key, data_dir.clone());

    // 3. 合成语速指令（伪精确：拖到 1.3x 即塞"请以约 1.3 倍速度朗读"）
    //    1.0（正常）不塞指令，保持默认音色自然度。
    let instruction: Option<String> = if (tts_speed - 1.0).abs() > 0.01 {
        Some(format!("请以约 {tts_speed} 倍速度朗读"))
    } else {
        None
    };

    // 4. TTS 生成
    let params = TTSParams {
        text: &text,
        voice: voice_string.as_deref(),
        instruction: instruction.as_deref(),
    };
    let audio_path = engine
        .generate(params)
        .await
        .map_err(|e| format!("{e}"))?;

    // 5. 保存消息记录
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
