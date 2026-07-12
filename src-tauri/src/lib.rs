// VoiceAssist Tauri 后端入口
// 按大纲 commands/ + storage/ + tts/ + tray.rs + hotkey.rs 模块逐步填充。

pub mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}