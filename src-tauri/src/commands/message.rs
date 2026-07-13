// 消息相关命令：列表、删除。

use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::Message;
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

/// 读取全部消息（按存储顺序）。
#[tauri::command]
pub fn list_messages(state: State<'_, AppState>) -> Vec<Message> {
    crate::storage::messages::load_messages(&state.data_dir)
}

/// 删除一条消息（连带：收藏来源置 None + 音频无引用则删）。
#[tauri::command]
pub fn delete_message(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<bool, String> {
    let data_dir = &state.data_dir;
    let result = crate::storage::messages::delete_message(data_dir, &id)
        .map_err(|e| format!("删除消息失败: {e}"))?;
    if result {
        notify_changed(&app, EVENT_MESSAGE_CHANGED);
    }
    Ok(result)
}
