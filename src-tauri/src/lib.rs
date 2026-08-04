// VoiceAssist Tauri 后端入口。
// 多窗口架构：main（主窗）+ quick_input（浮窗），跨窗口同步事件。

pub mod commands;
pub mod hotkey;
pub mod plugins;
pub mod storage;
pub mod sync;
pub mod tray;
pub mod tts;

use std::sync::Mutex;
use tauri::Manager;
use crate::commands::AppState;
use crate::hotkey::{FavoriteHotkeys, HotkeyState};
use crate::storage::types::Settings;

/// 显示并聚焦主窗口（从浮窗按钮调用），同时隐藏浮窗（避免两窗同现）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.show().map_err(|e| format!("{e}"))?;
        win.set_focus().map_err(|e| format!("{e}"))?;
    }
    if let Some(floating) = app.get_webview_window("quick_input") {
        let _ = floating.hide();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 已有实例被再次启动：把已存在的主窗拉前台
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            storage::ensure_data_dirs(&data_dir)?;
            let settings: Settings = storage::settings::load_settings(&data_dir);

            // 插件系统：加载已安装插件（单个插件失败只记日志，不影响主流程）
            app.manage(plugins::PluginManager::load_all(&data_dir));

            // 浮窗呼出快捷键：读设置 → 注册（失败只记日志，不影响主功能）
            let accel = settings.hotkey_show_window.clone();
            let register_ok = match hotkey::register_hotkey(app.handle(), &accel) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("注册快捷键 {accel} 失败: {e}");
                    false
                }
            };
            app.manage(HotkeyState {
                current: Mutex::new(register_ok.then_some(accel)),
            });
            app.manage(FavoriteHotkeys::new());

            // 加载收藏用于注册收藏快捷键（data_dir 随后移入 AppState）
            let favorites = storage::favorites::load_favorites(&data_dir);

            app.manage(AppState::new(data_dir, settings));
            // 虚拟麦克风播放控制（专用音频线程）
            app.manage(crate::commands::mic::MicPlayback::spawn());
            // 注册收藏快捷键（需在 AppState/MicPlayback manage 之后）
            if let Err(e) = hotkey::refresh_favorite_hotkeys(app.handle(), &favorites) {
                eprintln!("注册收藏快捷键失败: {e}");
            }
            // 系统托盘
            tray::setup(app)?;
            tray::install_close_to_tray(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::tts::generate_tts,
            crate::commands::tts::list_edge_voices,
            crate::commands::plugins::list_plugins,
            crate::commands::message::list_messages,
            crate::commands::message::delete_message,
            crate::commands::favorite::list_favorites,
            crate::commands::favorite::add_favorite,
            crate::commands::favorite::delete_favorite,
            crate::commands::favorite::import_favorite,
            crate::commands::favorite::set_favorite_hotkey,
            crate::commands::favorite::remove_favorite_hotkey,
            crate::commands::settings::get_settings,
            crate::commands::settings::update_setting,
            crate::commands::audio::resolve_audio_url,
            crate::commands::clone_voice::import_clone_voice,
            crate::commands::clone_voice::remove_clone_voice,
            crate::commands::mic::list_mic_devices,
            crate::commands::mic::check_vb_cable,
            crate::commands::mic::play_to_mic,
            crate::commands::mic::test_mic,
            crate::commands::mic::stop_mic,
            crate::commands::mic::get_mic_status,
            crate::hotkey::set_hotkey,
            crate::show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
