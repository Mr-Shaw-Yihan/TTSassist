// 悬浮球：常驻置顶小球的配套命令（不依赖全局快捷键的启动方式，游戏内可用）。
//
// 窗口本体在 tauri.conf.json 定义（label=floating_ball，56x56 透明置顶），
// 前端 FloatingBall 组件渲染小球与右键菜单；本模块提供它需要的能力：
// - toggle_quick_input：单击小球展开/收起快速输入浮窗（与呼出快捷键同逻辑）
// - toggle_mic_send：右键菜单「开关发送到麦克风」（与快捷键同逻辑）
// - set_floating_ball_enabled：设置页开关（持久化 + 窗口显隐）
// - save_floating_ball_pos：拖拽结束后保存位置（下次启动还原）
// - start/stop_outside_click_watch：菜单展开期间的「窗口外点击」检测（收菜单）
//
// 「播放最近一条消息」不需要命令：前端直接 emit "playback:play-last"，
// 与快捷键同一事件，主窗监听器统一处理扬声器 + 虚拟麦克风。

use tauri::{AppHandle, Manager, State};
use crate::commands::AppState;
use crate::hotkey;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

// ── 「窗口外点击」检测（右键菜单收回） ─────────────────────────────
//
// 悬浮球窗口 focus: false 免焦点，点击桌面/其他程序时 webview 收不到任何事件，
// 展开的右键菜单不会自动收回。这里用 GetAsyncKeyState 轻量轮询检测全局点击：
// 点击落在悬浮球窗口矩形外 → 广播事件让前端收菜单。
// 直接 #[link] 声明 Win32 API，不引入 windows-sys 依赖；只查按键状态不装钩子，不干扰游戏。
static OUTSIDE_WATCH_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const VK_LBUTTON: i32 = 0x01;
const VK_RBUTTON: i32 = 0x02;
const VK_MBUTTON: i32 = 0x04;

#[repr(C)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(v_key: i32) -> i16;
    fn GetCursorPos(lp_point: *mut WinPoint) -> i32;
}

/// 单击悬浮球：切换快速输入浮窗（与「呼出浮窗」快捷键完全同一条逻辑）
#[tauri::command]
pub fn toggle_quick_input(app: AppHandle) {
    hotkey::toggle_quick_input(&app);
}

/// 右键菜单「开关发送到麦克风」（与快捷键同逻辑）
#[tauri::command]
pub fn toggle_mic_send(app: AppHandle) {
    hotkey::toggle_mic_send(&app);
}

/// 设置页开关悬浮球：持久化 + 同步内存 + 显示/隐藏窗口 + 广播。
/// 不走前端 patch（update_setting 白名单也能存值，但不会动窗口——显隐必须在这里做）。
#[tauri::command]
pub fn set_floating_ball_enabled(
    app: AppHandle,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::settings::update_setting(
        &state.data_dir,
        "floating_ball_enabled",
        serde_json::json!(enabled),
    )
    .map_err(|e| format!("保存设置失败: {e}"))?;
    if let Ok(mut s) = state.settings.write() {
        s.floating_ball_enabled = enabled;
    }
    if let Some(ball) = app.get_webview_window("floating_ball") {
        let _ = if enabled { ball.show() } else { ball.hide() };
    }
    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(())
}

/// 拖拽结束后保存悬浮球位置（屏幕物理像素；前端 onMoved 防抖后调用）
#[tauri::command]
pub fn save_floating_ball_pos(
    x: i32,
    y: i32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    for (key, val) in [("floating_ball_x", x), ("floating_ball_y", y)] {
        crate::storage::settings::update_setting(
            &state.data_dir,
            key,
            serde_json::json!(val),
        )
        .map_err(|e| format!("保存悬浮球位置失败: {e}"))?;
    }
    if let Ok(mut s) = state.settings.write() {
        s.floating_ball_x = x;
        s.floating_ball_y = y;
    }
    Ok(())
}

/// 菜单展开时启动「窗口外点击」监视。
/// width/height 为展开后窗口的逻辑尺寸（前端按 ballPx + 菜单常量算好传入，
/// 不重读实际窗口尺寸——setSize 是异步的，此刻窗口可能还没变大）。
/// 检测到窗口外点击 → 广播 floating_ball:outside-click（前端收菜单）并自动停止。
#[tauri::command]
pub fn start_outside_click_watch(app: AppHandle, width: f64, height: f64) {
    use std::sync::atomic::Ordering;
    let Some(ball) = app.get_webview_window("floating_ball") else { return };
    let (Ok(pos), Ok(scale)) = (ball.outer_position(), ball.scale_factor()) else { return };
    // 已有监视在跑就不重复起（前端 effect 重入时先 stop 再 start，这里只是兜底）
    if OUTSIDE_WATCH_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let left = pos.x;
    let top = pos.y;
    let right = pos.x + (width * scale).round() as i32;
    let bottom = pos.y + (height * scale).round() as i32;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(40));
            if !OUTSIDE_WATCH_RUNNING.load(Ordering::SeqCst) {
                return;
            }
            // 低比特位 = 自上次调用后是否被按下（即新发生的点击）
            let clicked = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
                .iter()
                .any(|vk| unsafe { GetAsyncKeyState(*vk) } & 0x0001 != 0);
            if !clicked {
                continue;
            }
            let mut pt = WinPoint { x: 0, y: 0 };
            let inside = unsafe { GetCursorPos(&mut pt) } != 0
                && pt.x >= left
                && pt.x < right
                && pt.y >= top
                && pt.y < bottom;
            if !inside {
                use tauri::Emitter;
                let _ = app.emit("floating_ball:outside-click", ());
                OUTSIDE_WATCH_RUNNING.store(false, Ordering::SeqCst);
                return;
            }
        }
    });
}

/// 菜单收起 / 窗口销毁：停止窗口外点击监视
#[tauri::command]
pub fn stop_outside_click_watch() {
    OUTSIDE_WATCH_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
}
