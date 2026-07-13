// 跨窗口事件广播工具。
//
// 每次后端数据变化（新增/删除消息、收藏、设置变更）后，
// 在此广播一个空事件信号，各窗口收到后自行重读数据。
// 事件本身不带数据 payload——这是设计原则（参见大纲 4.6）。

pub const EVENT_MESSAGE_CHANGED: &str = "message:changed";
pub const EVENT_FAVORITE_CHANGED: &str = "favorite:changed";
pub const EVENT_SETTINGS_CHANGED: &str = "settings:changed";

/// 向所有窗口广播事件（空 payload，仅通知"数据变了"）。
/// 各窗口收到后应重新读取对应数据文件。
pub fn notify_changed(app: &tauri::AppHandle, event: &str) {
    use tauri::Emitter;
    let _ = app.emit(event, ());
}
