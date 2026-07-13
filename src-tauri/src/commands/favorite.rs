// 收藏相关命令：列表、新增、删除。
//
// 收藏策略（用户拍板）：仅引用源消息音频，不拷贝文件。
// 依靠 storage::audio_gc 的无引用则删逻辑保护音频不被误删。

use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::{Favorite, gen_id, now_iso};
use crate::sync::{notify_changed, EVENT_FAVORITE_CHANGED};

/// 读取全部收藏。
#[tauri::command]
pub fn list_favorites(state: State<'_, AppState>) -> Vec<Favorite> {
    crate::storage::favorites::load_favorites(&state.data_dir)
}

/// 从消息添加收藏：通过 source_message_id 找到原消息音频路径，
/// 仅在收藏中引用同一条音频文件（不拷贝）。
/// note 为必填备注（非空校验在 storage 层），source_message_id 必须对应一条消息。
#[tauri::command]
pub fn add_favorite(
    source_message_id: String,
    note: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Favorite, String> {
    let data_dir = &state.data_dir;

    // 查找源消息获取 audio_path
    let messages = crate::storage::messages::load_messages(data_dir);
    let msg = messages
        .iter()
        .find(|m| m.id == source_message_id)
        .ok_or_else(|| "找不到该消息，无法收藏".to_string())?;

    let fav = Favorite {
        id: gen_id("f"),
        source_message_id: Some(source_message_id),
        note,
        audio_path: msg.audio_path.clone(),
        created_at: now_iso(),
    };

    let result = fav.clone();
    crate::storage::favorites::add_favorite(data_dir, fav)
        .map_err(|e| format!("{e}"))?;

    notify_changed(&app, EVENT_FAVORITE_CHANGED);

    Ok(result)
}

/// 删除一条收藏（存储层自动处理：收藏删除后音频无引用则清除）。
#[tauri::command]
pub fn delete_favorite(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let data_dir = &state.data_dir;
    let result = crate::storage::favorites::delete_favorite(data_dir, &id)
        .map_err(|e| format!("删除收藏失败: {e}"))?;
    if result {
        notify_changed(&app, EVENT_FAVORITE_CHANGED);
    }
    Ok(result)
}
