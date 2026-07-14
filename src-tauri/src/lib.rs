// VoiceAssist Tauri 后端入口。
// 逐步按大纲 commands/ + storage/ + tts/ + sync/ + tray/ + hotkey/ 填充。

pub mod commands;
pub mod storage;
pub mod sync;
pub mod tts;

use tauri::Manager;
use crate::commands::AppState;
use crate::storage::types::Settings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            storage::ensure_data_dirs(&data_dir)?;
            let settings: Settings = storage::settings::load_settings(&data_dir);
            app.manage(AppState::new(data_dir, settings));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::tts::generate_tts,
            crate::commands::message::list_messages,
            crate::commands::message::delete_message,
            crate::commands::favorite::list_favorites,
            crate::commands::favorite::add_favorite,
            crate::commands::favorite::delete_favorite,
            crate::commands::settings::get_settings,
            crate::commands::settings::update_setting,
            crate::commands::audio::resolve_audio_url,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
