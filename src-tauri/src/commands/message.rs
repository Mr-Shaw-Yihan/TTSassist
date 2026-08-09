// 消息相关命令：列表、删除。

use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::Message;
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

/// 消息分页结果：窗口消息（旧→新）+ 前面是否还有更早的
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub has_more: bool,
}

/// 分页读消息：取 before_id（不含）之前的最近 limit 条。
/// 前端首屏不传参取最新一页，上滑翻页时传当前最早一条的 id。
#[tauri::command]
pub fn list_messages(
    limit: Option<usize>,
    before_id: Option<String>,
    state: State<'_, AppState>,
) -> MessagePage {
    let (messages, has_more) = crate::storage::messages::load_messages_page(
        &state.data_dir,
        limit,
        before_id.as_deref(),
    );
    MessagePage { messages, has_more }
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
