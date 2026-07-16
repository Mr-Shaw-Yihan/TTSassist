// VoiceAssist Tauri 后端入口。
// 多窗口架构：main（主窗）+ quick_input（浮窗），跨窗口同步事件。

pub mod commands;
pub mod storage;
pub mod sync;
pub mod tts;

use tauri::Manager;
use crate::commands::AppState;
use crate::storage::types::Settings;

/// 全局快捷键：切换浮窗显隐
fn setup_hotkey(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcut_key = "Alt+V";

    let _ = app.global_shortcut().on_shortcut(shortcut_key, move |_app, _event, _state| {
        if let Some(win) = _app.get_webview_window("quick_input") {
            if win.is_visible().unwrap_or(false) {
                let _ = win.hide();
            } else {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    });
}

/// 显示并聚焦主窗口（从浮窗菜单调用）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.show().map_err(|e| format!("{e}"))?;
        win.set_focus().map_err(|e| format!("{e}"))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            storage::ensure_data_dirs(&data_dir)?;
            let settings: Settings = storage::settings::load_settings(&data_dir);
            app.manage(AppState::new(data_dir, settings));
            // 注册快捷键
            setup_hotkey(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::tts::generate_tts,
            crate::commands::message::list_messages,
            crate::commands::message::delete_message,
            crate::commands::favorite::list_favorites,
            crate::commands::favorite::add_favorite,
            crate::commands::favorite::delete_favorite,
            crate::commands::favorite::import_favorite,
            crate::commands::settings::get_settings,
            crate::commands::settings::update_setting,
            crate::commands::audio::resolve_audio_url,
            crate::commands::clone_voice::import_clone_voice,
            crate::commands::clone_voice::remove_clone_voice,
            crate::show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
