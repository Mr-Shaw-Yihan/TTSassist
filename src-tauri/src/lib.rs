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
pub mod win32;

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

            // 宿主能力桥：先于插件加载 manage——插件 attach 期间即可调用
            // get_state / subscribe_events（桥的播放态聚合监听也在此注册）
            let host_bridge = plugins::HostBridge::new();
            host_bridge.setup_playback_listeners(app.handle());
            app.manage(host_bridge);

            // AppState / MicPlayback 提前 manage：能力桥包装的合成、收藏播放、
            // 停止播放等能力依赖二者，插件 attach 时必须已就绪
            app.manage(AppState::new(data_dir.clone(), settings.clone()));
            app.manage(crate::commands::mic::MicPlayback::spawn());

            // 阶段 22：插件根目录改为 exe 同级 plugins/（脱离 APPDATA 系统盘）
            let plugins_root = resolve_plugins_root();
            migrate_plugins_if_needed(&data_dir, &plugins_root);

            // 插件系统：加载已安装插件（单个插件失败只记日志，不影响主流程）；
            // 传入 AppHandle 供宿主能力桥注入（声明 requires_host_bridge 的插件）
            let plugin_manager = plugins::PluginManager::load_all(&plugins_root, Some(app.handle()));
            // 通用插件配置注入：按各插件 manifest 的 config 声明把
            // settings.plugin_config 注入环境变量（替代旧 minimax 硬编码注入）
            plugin_manager.inject_config_env(&settings.plugin_config);
            app.manage(plugin_manager);
            // 补注入 load_all 期间挂起的能力桥（attach 回调需取到已 manage 的 PluginManager）
            app.state::<plugins::PluginManager>().attach_pending_bridges();

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

            // 悬浮球启动序列：球窗先出播 progress（主窗 visible=false 延迟到 va:boot:done）。
            // 位置：有保存值用保存值，否则主显示器居中；窗口尺寸 = 球径×1.5 画布。
            // 启动后是否收球由前端按 floating_ball_enabled 决定（见 FloatingBall.tsx boot 控制器）
            if let Some(ball) = app.get_webview_window("floating_ball") {
                let ball_px = storage::types::clamp_ball_size(settings.floating_ball_size);
                let scale = app
                    .primary_monitor()
                    .ok()
                    .flatten()
                    .map(|m| m.scale_factor())
                    .unwrap_or(1.0);
                let canvas_phys = ((ball_px as f64) * 1.5 * scale) as i32;
                let (mut x, mut y) = (settings.floating_ball_x, settings.floating_ball_y);
                if x < 0 || y < 0 {
                    if let Ok(Some(mon)) = app.primary_monitor() {
                        let size = mon.size();
                        x = (size.width as i32 - canvas_phys) / 2;
                        y = (size.height as i32 - canvas_phys) / 2;
                    }
                }
                if x >= 0 && y >= 0 {
                    let _ = ball.set_position(tauri::PhysicalPosition::new(x, y));
                }
                let canvas_log = (ball_px as f64 * 1.5) as u32;
                let _ = ball.set_size(tauri::LogicalSize::new(canvas_log, canvas_log));
                // 悬浮球点击不激活窗口、不抢游戏焦点（config 的 focus:false 只管创建时机，不够）
                crate::win32::set_no_activate(&ball, true);
                let _ = ball.show();
            }

            // 焦点诊断：记录 quick_input 焦点变化与当时的前台窗口（排查游戏内浮窗自动消失）
            if let Some(qi) = app.get_webview_window("quick_input") {
                // 直播伴侣形态：浮窗永久挂 WS_EX_NOACTIVATE——点击只送达鼠标消息、
                // 不激活窗口，前台永远是游戏；打字靠 SetFocus webview 子窗口建立键盘路由。
                crate::win32::set_no_activate(&qi, true);
                let log_dir = data_dir.clone(); // data_dir 之后还要移入 AppState，先克隆
                qi.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(focused) = event {
                        let (hwnd, title) = crate::win32::foreground_info();
                        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                        let line = format!("[{ts}] quick_input focused={focused}，当时前台窗口: \"{title}\" (hwnd=0x{hwnd:X})");
                        eprintln!("{line}");
                        use std::io::Write;
                        let path = log_dir.join("focus_debug.log");
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                            let _ = writeln!(f, "{line}");
                        }
                    }
                });
            }

            // AppState / MicPlayback 已在插件加载前 manage（宿主能力桥依赖），此处不再注册

            // 注册收藏快捷键（需在 AppState/MicPlayback manage 之后）
            if let Err(e) = hotkey::refresh_favorite_hotkeys(app.handle(), &favorites) {
                eprintln!("注册收藏快捷键失败: {e}");
            }
            // 系统托盘
            tray::setup(app)?;
            tray::install_close_to_tray(app.handle());
            // 后端初始化完成 → 通知球窗前端（boot 控制器等 max(ready, 800ms) 后收球开主窗）
            {
                crate::commands::floating_ball::mark_boot_ready();
                use tauri::Emitter;
                let _ = app.handle().emit("va:boot:ready", ());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::tts::generate_tts,
            crate::commands::plugins::list_plugins,
            crate::commands::plugins::uninstall_plugin,
            crate::commands::plugins::get_plugin_config,
            crate::commands::plugins::set_plugin_config,
            crate::commands::plugins::clear_plugin_config,
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
                        crate::commands::floating_ball::toggle_quick_input,
            crate::hotkey::focus_quick_input_content,
            crate::commands::floating_ball::toggle_mic_send,
            crate::commands::floating_ball::set_floating_ball_enabled,
            crate::commands::floating_ball::save_floating_ball_pos,
            crate::commands::floating_ball::start_outside_click_watch,
            crate::commands::floating_ball::stop_outside_click_watch,
            crate::commands::floating_ball::reset_floating_ball_pos,
            crate::commands::floating_ball::is_boot_ready,
            crate::commands::floating_ball::start_cursor_watch,
            crate::commands::floating_ball::stop_cursor_watch,
            crate::commands::floating_ball::update_ball_hit_rect,
            crate::commands::floating_ball::set_ball_passthrough_override,
            crate::show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
