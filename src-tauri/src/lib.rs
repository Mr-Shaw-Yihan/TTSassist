// VoiceAssist Tauri 后端入口。
// 多窗口架构：main（主窗）+ quick_input（浮窗），跨窗口同步事件。

pub mod asr;
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

/// 解析插件根目录（阶段 22：脱离 APPDATA，跟随 exe 安装位置）。
/// 优先级：环境变量 VA_PLUGINS_DIR > exe 同级 plugins/ 目录。
fn resolve_plugins_root() -> std::path::PathBuf {
    // 1. 环境变量覆盖（开发/测试/高级用户自定义）
    if let Ok(dir) = std::env::var("VA_PLUGINS_DIR") {
        return std::path::PathBuf::from(dir);
    }
    // 2. 默认：exe 所在目录的 plugins/ 子目录
    let exe = std::env::current_exe().expect("无法获取 exe 路径");
    exe.parent()
        .expect("exe 无父目录")
        .join("plugins")
}

/// 首次运行迁移：把旧 APPDATA 下的插件复制到新位置（exe 同级 plugins/）。
/// 旧目录保留不删（安全起见）；新位置已有 registry.json 则跳过（避免覆盖）。
fn migrate_plugins_if_needed(data_dir: &std::path::Path, plugins_root: &std::path::Path) {
    let old_root = data_dir.join("plugins");
    let old_registry = old_root.join("registry.json");
    let new_registry = plugins_root.join("registry.json");

    // 旧位置没插件，或新位置已有注册表 → 无需迁移
    if !old_registry.exists() || new_registry.exists() {
        return;
    }

    eprintln!("检测到旧位置插件，正在迁移: {} → {}", old_root.display(), plugins_root.display());
    if let Err(e) = copy_dir_all(&old_root, plugins_root) {
        eprintln!("插件迁移失败（不影响启动，可手动复制）: {e}");
    } else {
        eprintln!("插件迁移完成。旧目录保留在 {}，确认无误后可手动删除。", old_root.display());
    }
}

/// 递归复制目录（迁移用）
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

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

            // 兼容迁移：老设置的内置 edge 引擎 → edge-tts 插件（幂等，直接改文件）
            plugins::migrate_legacy_engine(&data_dir);

            let settings: Settings = storage::settings::load_settings(&data_dir);

            // ASR 插件需要 API Key：通过环境变量传递（插件加载时读取）
            if !settings.mimo_api_key.is_empty() {
                std::env::set_var("MIMO_API_KEY", &settings.mimo_api_key);
            }

            // MiniMax 插件环境变量注入（插件通过 std::env::var 读取 API Key）
            if !settings.minimax_api_key.is_empty() {
                std::env::set_var("MINIMAX_API_KEY", &settings.minimax_api_key);
            }
            if !settings.minimax_global_api_key.is_empty() {
                std::env::set_var("MINIMAX_GLOBAL_API_KEY", &settings.minimax_global_api_key);
            }

            // 阶段 22：插件根目录改为 exe 同级 plugins/（脱离 APPDATA 系统盘）
            let plugins_root = resolve_plugins_root();
            migrate_plugins_if_needed(&data_dir, &plugins_root);

            // 插件系统：加载已安装插件（单个插件失败只记日志，不影响主流程）
            app.manage(plugins::PluginManager::load_all(&plugins_root));

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

            // 语音输入快捷键（按住说话）：非空才注册，失败只记日志
            let vi_accel = settings.voice_input_hotkey.clone();
            let vi_ok = if vi_accel.is_empty() {
                false
            } else {
                match hotkey::register_voice_input_hotkey(app.handle(), &vi_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("注册语音输入快捷键 {vi_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::VoiceInputHotkeyState {
                current: Mutex::new(vi_ok.then_some(vi_accel)),
            });

            // 播放最近一条消息 / 开关麦克风快捷键：非空才注册，失败只记日志
            let pl_accel = settings.hotkey_play_last.clone();
            let pl_ok = if pl_accel.is_empty() {
                false
            } else {
                match hotkey::register_play_last_hotkey(app.handle(), &pl_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("注册播放最近消息快捷键 {pl_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::PlayLastHotkeyState {
                current: Mutex::new(pl_ok.then_some(pl_accel)),
            });

            let mt_accel = settings.hotkey_mic_toggle.clone();
            let mt_ok = if mt_accel.is_empty() {
                false
            } else {
                match hotkey::register_mic_toggle_hotkey(app.handle(), &mt_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("注册麦克风开关快捷键 {mt_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::MicToggleHotkeyState {
                current: Mutex::new(mt_ok.then_some(mt_accel)),
            });

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
            crate::commands::plugins::list_plugins,
            crate::commands::plugins::uninstall_plugin,
            crate::commands::plugins::install_plugin_zip,
            crate::commands::plugins::import_offline_resources,
            crate::commands::plugins::clean_failed_resources,
            crate::commands::plugins::fetch_plugin_index,
            crate::commands::plugins::download_install_plugin,
            crate::commands::plugins::list_bundled_plugins,
            crate::commands::plugins::install_bundled_plugin,
            crate::commands::plugins::run_plugin_setup,
            crate::commands::plugins::install_voice,
            crate::commands::plugins::uninstall_voice,
            crate::commands::plugins::preload_voice,
            crate::commands::plugins::import_voice_pack,
            crate::commands::update::check_app_update,
            crate::commands::remote::get_remote_config,
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
            crate::commands::vbcable::download_vb_cable,
            crate::commands::vbcable::install_vb_cable,
            crate::asr::list_asr_plugins,
            crate::asr::asr_transcribe,
            crate::commands::minimax_clone::minimax_global_voice_clone,
            crate::commands::minimax_clone::minimax_global_get_voices,
            crate::commands::minimax_clone::minimax_global_delete_voice,
            crate::hotkey::set_hotkey,
            crate::hotkey::set_voice_input_hotkey,
            crate::hotkey::set_play_last_hotkey,
            crate::hotkey::set_mic_toggle_hotkey,
            crate::show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
