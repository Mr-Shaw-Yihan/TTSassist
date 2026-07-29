// 浮窗呼出全局快捷键：注册 / 注销 / 自定义。
//
// 关键点：
// - 回调从 handler 第一个参数拿 &AppHandle（无需闭包捕获），切换 quick_input 显隐。
// - set_hotkey 先注册新快捷键（验证有效）再注销旧的——避免"新快捷键无效→旧的也没了"。

use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// 记录当前已注册的快捷键（用于切换时注销旧的）
pub struct HotkeyState {
    pub current: Mutex<Option<String>>,
}

/// 注册一个全局快捷键，回调切换 quick_input 窗口显隐。
pub fn register_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            // 只响应按下事件
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if let Some(win) = app.get_webview_window("quick_input") {
                if win.is_visible().unwrap_or(false) {
                    let _ = win.hide();
                } else {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        })
        .map_err(|e| format!("注册快捷键失败：{e}"))
}

/// 设置（更换）全局快捷键命令。
///
/// 流程：验证非空 → 与当前相同则直接返回 → 注册新（失败则旧的保持）→ 注销旧 → 持久化。
#[tauri::command]
pub fn set_hotkey(
    app: AppHandle,
    accel: String,
    hotkey: State<'_, HotkeyState>,
    app_state: State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    let accel = accel.trim().to_string();
    if accel.is_empty() {
        return Err("快捷键不能为空".into());
    }

    // 当前已注册的快捷键
    let current = hotkey
        .current
        .lock()
        .map(|g| g.clone())
        .unwrap_or(None);

    // 无变化则直接返回
    if current.as_deref() == Some(accel.as_str()) {
        return Ok(());
    }

    // 1. 先注册新快捷键（验证有效；失败则旧的保持不动）
    register_hotkey(&app, &accel)?;

    // 2. 注销旧快捷键
    if let Some(old) = current.as_ref() {
        if old != &accel {
            let _ = app.global_shortcut().unregister(old.as_str());
        }
    }

    // 3. 更新 HotkeyState
    if let Ok(mut g) = hotkey.current.lock() {
        *g = Some(accel.clone());
    }

    // 4. 持久化 + 同步内存 settings
    crate::storage::settings::update_setting(
        &app_state.data_dir,
        "hotkey_show_window",
        serde_json::json!(accel),
    )
    .map_err(|e| format!("保存快捷键失败：{e}"))?;
    if let Ok(mut g) = app_state.settings.write() {
        g.hotkey_show_window = accel;
    }

    Ok(())
}
