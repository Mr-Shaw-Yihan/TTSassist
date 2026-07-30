// 浮窗呼出全局快捷键：注册 / 注销 / 自定义。
//
// 关键点：
// - 回调从 handler 第一个参数拿 &AppHandle（无需闭包捕获），切换 quick_input 显隐。
// - set_hotkey 先注册新快捷键（验证有效）再注销旧的——避免"新快捷键无效→旧的也没了"。

use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use crate::commands::mic::MicPlayback;
use crate::commands::AppState;
use crate::storage::types::Favorite;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 记录当前已注册的快捷键（用于切换时注销旧的）
pub struct HotkeyState {
    pub current: Mutex<Option<String>>,
}

/// 记录已注册的收藏快捷键（用于刷新时先全部注销再重注册）
pub struct FavoriteHotkeys {
    pub registered: Mutex<HashSet<String>>,
}

impl FavoriteHotkeys {
    pub fn new() -> Self {
        Self { registered: Mutex::new(HashSet::new()) }
    }
}

/// 注册一个全局快捷键，回调切换 quick_input 窗口显隐。
/// 呼出浮窗时隐藏主窗（避免两窗同现）；收起浮窗时主窗保持隐藏（用户自行通过托盘/浮窗按钮打开）。
pub fn register_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            // 只响应按下事件
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if let Some(floating) = app.get_webview_window("quick_input") {
                if floating.is_visible().unwrap_or(false) {
                    // 浮窗已显示 → 收起浮窗（主窗保持隐藏）
                    let _ = floating.hide();
                } else {
                    // 呼出浮窗，同时隐藏主窗（避免两窗同现）
                    let _ = floating.show();
                    let _ = floating.set_focus();
                    if let Some(main) = app.get_webview_window("main") {
                        let _ = main.hide();
                    }
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

    // 广播 settings:changed，让前端 store 刷新（与其它设置项一致）
    notify_changed(&app, EVENT_SETTINGS_CHANGED);

    Ok(())
}

/// 注册一个收藏快捷键：按下时发麦克风（若开启）+ emit 事件让前端播扬声器。
fn register_favorite_hotkey(app: &AppHandle, hotkey: &str, audio_path: String) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(hotkey, move |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            // 发麦克风（若全局开关开启且配置了设备）
            if let Some(state) = app.try_state::<AppState>() {
                let abs = state.data_dir.join(&audio_path);
                let (enabled, device, volume) = match state.settings.read() {
                    Ok(s) => (s.mic_send_enabled, s.mic_output_device.clone(), s.mic_playback_volume),
                    Err(_) => (false, String::new(), 1.0),
                };
                if enabled && !device.is_empty() {
                    if let Some(mic) = app.try_state::<MicPlayback>() {
                        mic.play(abs, device, volume);
                    }
                }
            }
            // emit 事件让前端主窗播扬声器
            let _ = app.emit("favorite:play", audio_path.clone());
        })
        .map_err(|e| format!("注册收藏快捷键失败：{e}"))
}

/// 刷新所有收藏快捷键：先注销已注册的全部收藏快捷键，再为所有带快捷键的收藏重新注册。
/// 不影响浮窗快捷键（那个由 HotkeyState 单独管理）。
pub fn refresh_favorite_hotkeys(app: &AppHandle, favorites: &[Favorite]) -> Result<(), String> {
    let state = app
        .try_state::<FavoriteHotkeys>()
        .ok_or("FavoriteHotkeys 状态未注册")?;
    let mut registered = state
        .registered
        .lock()
        .map_err(|e| format!("锁失败：{e}"))?;

    // 先注销所有已注册的收藏快捷键
    for hk in registered.iter() {
        let _ = app.global_shortcut().unregister(hk.as_str());
    }
    registered.clear();

    // 为所有带快捷键的收藏重新注册
    for fav in favorites {
        if let Some(hk) = &fav.hotkey {
            register_favorite_hotkey(app, hk, fav.audio_path.clone())?;
            registered.insert(hk.clone());
        }
    }
    Ok(())
}
