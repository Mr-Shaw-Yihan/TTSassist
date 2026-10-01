// 音频路径解析命令：把相对 audio 路径转成绝对路径，供前端 convertFileSrc 用。
//
// 本命令是前端播放链的**唯一咽喉**（getAudioUrl / getAudioAbsPath / revealAudio /
// playToMic 四条链都经它），主窗 / 快速输入 / 字幕 / 悬浮球四窗口 + 虚拟麦克风 + 遥控全覆盖。

use tauri::{AppHandle, State};
use crate::commands::AppState;

/// 接收 messages/favorites 中存的相对音频路径（如 "audio/m_xxx.wav"），
/// 拼上 data_dir 返回绝对路径。
///
/// `_apply_fx`：历史参数（效果器已移除），保留以兼容前端既有调用（revealAudio 显式传 false）。
#[tauri::command]
pub async fn resolve_audio_url(
    rel_path: String,
    _apply_fx: Option<bool>,
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    Ok(state.data_dir.join(&rel_path).to_string_lossy().into_owned())
}
