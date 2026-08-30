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

/// 记录当前已注册的语音输入快捷键（用于切换时注销旧的）
pub struct VoiceInputHotkeyState {
    pub current: Mutex<Option<String>>,
}

/// 记录当前已注册的「播放最近一条消息」快捷键
pub struct PlayLastHotkeyState {
    pub current: Mutex<Option<String>>,
}

/// 记录当前已注册的「开关发送到麦克风」快捷键
pub struct MicToggleHotkeyState {
    pub current: Mutex<Option<String>>,
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
            toggle_quick_input(app);
        })
        .map_err(|e| format!("注册快捷键失败：{e}"))
}

/// 切换快速输入浮窗显隐（呼出快捷键与悬浮球点击共用）。
/// 呼出浮窗时隐藏主窗（避免两窗同现）；收起浮窗时主窗保持隐藏（用户自行通过托盘/浮窗按钮打开）。
pub fn toggle_quick_input(app: &AppHandle) {
    if let Some(floating) = app.get_webview_window("quick_input") {
        if floating.is_visible().unwrap_or(false) {
            // 浮窗已显示 → 收起浮窗（主窗保持隐藏）
            let _ = floating.hide();
        } else {
            // 呼出浮窗，同时隐藏主窗（避免两窗同现）。
            // 无激活呼出：不抢前台焦点（全屏/无边框游戏不被打断）；失败才回退普通 show()。
            // 用户点击输入框后由前端调 focus_quick_input_content 建立键盘路由。
            if !crate::win32::show_no_activate(&floating) {
                let _ = floating.show();
            }
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.hide();
            }
        }
    }
}

/// 直播伴侣形态：把键盘焦点交给浮窗 webview 子窗口（不激活窗口、前台仍是游戏）。
/// 前端点击输入框后调用，此后打字与 ESC 关闭可用。
#[tauri::command]
pub fn focus_quick_input_content(app: AppHandle) {
    if let Some(qi) = app.get_webview_window("quick_input") {
        crate::win32::focus_webview_child(&qi);
    }
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

    // 0. 冲突检测：与其它快捷键项（含收藏）重复时拒绝
    if let Some(name) = find_accel_conflict(&app_state, &accel, Some("hotkey_show_window")) {
        return Err(format!("快捷键 {accel} 已被「{name}」占用，请先清除或换一个"));
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

/// 注册语音输入全局快捷键：按住说话模式。
/// 按下 emit "voice-input:pressed"，松开 emit "voice-input:released"，前端可见窗口接管录音会话。
pub fn register_voice_input_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            match event.state() {
                ShortcutState::Pressed => {
                    let _ = app.emit("voice-input:pressed", ());
                }
                ShortcutState::Released => {
                    let _ = app.emit("voice-input:released", ());
                }
            }
        })
        .map_err(|e| format!("注册语音输入快捷键失败：{e}"))
}

/// 设置（更换/清除）语音输入快捷键。空串 = 注销并清除。
///
/// 流程：无变化直接返回 → 注册新（非空；失败则旧的保持）→ 注销旧 → 持久化。
#[tauri::command]
pub fn set_voice_input_hotkey(
    app: AppHandle,
    accel: String,
    state: State<'_, VoiceInputHotkeyState>,
    app_state: State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    let accel = accel.trim().to_string();

    let current = state
        .current
        .lock()
        .map(|g| g.clone())
        .unwrap_or(None);
    if current.as_deref() == Some(accel.as_str()) || (accel.is_empty() && current.is_none()) {
        return Ok(());
    }

    // 冲突检测：与其它快捷键项（含收藏）重复时拒绝
    if !accel.is_empty() {
        if let Some(name) = find_accel_conflict(&app_state, &accel, Some("voice_input_hotkey")) {
            return Err(format!("快捷键 {accel} 已被「{name}」占用，请先清除或换一个"));
        }
    }

    // 1. 先注册新快捷键（验证有效；失败则旧的保持不动）
    if !accel.is_empty() {
        register_voice_input_hotkey(&app, &accel)?;
    }

    // 2. 注销旧快捷键
    if let Some(old) = current.as_ref() {
        if old != &accel {
            let _ = app.global_shortcut().unregister(old.as_str());
        }
    }

    // 3. 更新状态
    if let Ok(mut g) = state.current.lock() {
        *g = if accel.is_empty() { None } else { Some(accel.clone()) };
    }

    // 4. 持久化 + 同步内存 settings + 广播
    crate::storage::settings::update_setting(
        &app_state.data_dir,
        "voice_input_hotkey",
        serde_json::json!(accel),
    )
    .map_err(|e| format!("保存快捷键失败：{e}"))?;
    if let Ok(mut g) = app_state.settings.write() {
        g.voice_input_hotkey = accel;
    }
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

/// 注册「播放最近一条消息」快捷键：按下时通知前端播最近一条消息的音频
/// （扬声器 + 开关开启时发虚拟麦克风，均由前端 playAudioWithMic 处理）。
pub fn register_play_last_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let _ = app.emit("playback:play-last", ());
        })
        .map_err(|e| format!("注册播放最近消息快捷键失败：{e}"))
}

/// 注册「开关发送到麦克风」快捷键：按下时翻转 mic_send_enabled（持久化 + 通知前端刷新）。
pub fn register_mic_toggle_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            toggle_mic_send(app);
        })
        .map_err(|e| format!("注册麦克风开关快捷键失败：{e}"))
}

/// 翻转「发送到麦克风」全局开关（快捷键与悬浮球菜单共用）：
/// 翻转内存态 → 持久化 → 广播 settings:changed 让前端刷新。
pub fn toggle_mic_send(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let new_val = match state.settings.write() {
        Ok(mut s) => {
            s.mic_send_enabled = !s.mic_send_enabled;
            s.mic_send_enabled
        }
        Err(_) => return,
    };
    if let Err(e) = crate::storage::settings::update_setting(
        &state.data_dir,
        "mic_send_enabled",
        serde_json::json!(new_val),
    ) {
        eprintln!("持久化麦克风开关失败: {e}");
    }
    notify_changed(app, EVENT_SETTINGS_CHANGED);
}

/// 检测快捷键是否已被其它项占用（四项全局快捷键 + 全部收藏），返回冲突方名称。
/// exclude_key：正在设置的设置项自身（跳过，不算冲突）。
pub fn find_accel_conflict(app_state: &AppState, accel: &str, exclude_key: Option<&str>) -> Option<String> {
    let s = match app_state.settings.read() {
        Ok(s) => s.clone(),
        Err(_) => return None,
    };
    if exclude_key != Some("hotkey_show_window") && s.hotkey_show_window == accel {
        return Some("呼出浮窗".to_string());
    }
    if exclude_key != Some("voice_input_hotkey") && !s.voice_input_hotkey.is_empty() && s.voice_input_hotkey == accel {
        return Some("语音输入".to_string());
    }
    if exclude_key != Some("hotkey_play_last") && !s.hotkey_play_last.is_empty() && s.hotkey_play_last == accel {
        return Some("播放最近一条消息".to_string());
    }
    if exclude_key != Some("hotkey_mic_toggle") && !s.hotkey_mic_toggle.is_empty() && s.hotkey_mic_toggle == accel {
        return Some("开关发送到麦克风".to_string());
    }
    let favorites = crate::storage::favorites::load_favorites(&app_state.data_dir);
    favorites
        .iter()
        .find(|f| f.hotkey.as_deref() == Some(accel))
        .map(|f| format!("收藏「{}」", f.note))
}

/// 通用「设置/清除快捷键」流程（供可清除的快捷键共用）：
/// 无变化直接返回 → 冲突检测 → 注册新（非空；失败则旧的保持）→ 注销旧 → 更新状态 → 持久化 + 同步内存 + 广播。
fn replace_hotkey(
    app: &AppHandle,
    state_lock: &Mutex<Option<String>>,
    accel: &str,
    setting_key: &str,
    register: fn(&AppHandle, &str) -> Result<(), String>,
    app_state: &AppState,
) -> Result<(), String> {
    let current = state_lock
        .lock()
        .map(|g| g.clone())
        .unwrap_or(None);
    if current.as_deref() == Some(accel) || (accel.is_empty() && current.is_none()) {
        return Ok(());
    }

    // 1. 冲突检测：与其它快捷键项（含收藏）重复时拒绝，避免同一组合键触发多个动作
    if !accel.is_empty() {
        if let Some(name) = find_accel_conflict(app_state, accel, Some(setting_key)) {
            return Err(format!("快捷键 {accel} 已被「{name}」占用，请先清除或换一个"));
        }
    }

    // 2. 先注册新快捷键（验证有效；失败则旧的保持不动）
    if !accel.is_empty() {
        register(app, accel)?;
    }

    // 3. 注销旧快捷键
    if let Some(old) = current.as_ref() {
        if old != accel {
            let _ = app.global_shortcut().unregister(old.as_str());
        }
    }

    // 4. 更新状态
    if let Ok(mut g) = state_lock.lock() {
        *g = if accel.is_empty() { None } else { Some(accel.to_string()) };
    }

    // 5. 持久化 + 同步内存 settings + 广播
    crate::storage::settings::update_setting(
        &app_state.data_dir,
        setting_key,
        serde_json::json!(accel),
    )
    .map_err(|e| format!("保存快捷键失败：{e}"))?;
    if let Ok(mut g) = app_state.settings.write() {
        match setting_key {
            "hotkey_play_last" => g.hotkey_play_last = accel.to_string(),
            "hotkey_mic_toggle" => g.hotkey_mic_toggle = accel.to_string(),
            _ => {}
        }
    }
    notify_changed(app, EVENT_SETTINGS_CHANGED);

    Ok(())
}

/// 设置（更换/清除）「播放最近一条消息」快捷键。空串 = 注销并清除。
#[tauri::command]
pub fn set_play_last_hotkey(
    app: AppHandle,
    accel: String,
    state: State<'_, PlayLastHotkeyState>,
    app_state: State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    replace_hotkey(
        &app,
        &state.current,
        accel.trim(),
        "hotkey_play_last",
        register_play_last_hotkey,
        &app_state,
    )
}

/// 设置（更换/清除）「开关发送到麦克风」快捷键。空串 = 注销并清除。
#[tauri::command]
pub fn set_mic_toggle_hotkey(
    app: AppHandle,
    accel: String,
    state: State<'_, MicToggleHotkeyState>,
    app_state: State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    replace_hotkey(
        &app,
        &state.current,
        accel.trim(),
        "hotkey_mic_toggle",
        register_mic_toggle_hotkey,
        &app_state,
    )
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
