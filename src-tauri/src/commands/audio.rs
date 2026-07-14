// 音频路径解析命令：把相对 audio 路径转成绝对路径，供前端 convertFileSrc 用。

use tauri::State;
use crate::commands::AppState;

/// 接收 messages/favorites 中存的相对音频路径（如 "audio/m_xxx.wav"），
/// 拼上 data_dir 返回绝对路径。
#[tauri::command]
pub fn resolve_audio_url(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let abs = state.data_dir.join(&rel_path);
    Ok(abs.to_string_lossy().into_owned())
}