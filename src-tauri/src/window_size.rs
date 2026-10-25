// 主窗口启动尺寸：纯函数 + 首帧应用。
// 主窗在 tauri.conf.json 以 visible=false 创建，本模块在显示前按设置里的记忆尺寸
// 定形后再显示，消除“先按配置默认画出来、前端再 setSize 变形”的跳动。
// 记忆值是物理像素（前端 innerSize），夹紧只防异常值，正常值原样应用。

use crate::storage::types::Settings;

/// 与前端旧守卫同口径：物理边长低于此值即视为窗口未布局时的占位值。
const MIN_PLAUSIBLE_SIDE: u32 = 200;

/// 计算启动窗口尺寸（物理像素）。
/// - 无记忆（0×0）或任一边是占位小值 → None，保持 tauri.conf.json 默认尺寸
/// - 正常值 → Some，并夹到 [min, max]，防止坏存档/换小屏后窗口超出屏幕
pub fn initial_window_size(
    settings: &Settings,
    min: (u32, u32),
    max: (u32, u32),
) -> Option<(u32, u32)> {
    let (w, h) = (settings.window_width, settings.window_height);
    if w < MIN_PLAUSIBLE_SIDE || h < MIN_PLAUSIBLE_SIDE {
        return None;
    }
    // max < min 只可能来自异常显示器信息；夹到合法区间避免 u32::clamp 断言崩溃
    Some((
        w.clamp(min.0, max.0.max(min.0)),
        h.clamp(min.1, max.1.max(min.1)),
    ))
}

/// 首帧底色（RGB）：窗口 show 到 WebView 加载完 index.html 之间会露出 WebView2 的
/// 默认背景（白），夜间用户因此先闪一帧日间。这里给出与记忆皮肤一致的底色，
/// 与 index.html 内联脚本、前端 CSS 的 body 底色三者同值（B2 真机实测）。
pub fn startup_background_rgb(theme: &str) -> (u8, u8, u8) {
    if theme == "dark" {
        (28, 27, 31)
    } else {
        (250, 248, 244)
    }
}

/// 主窗显示兜底：前端若异常（首帧就绪信号发不出来），到点也必须把窗口露出来，
/// 否则用户看到的是“进程在跑但没窗口”。
const SHOW_FALLBACK_MS: u64 = 3000;

/// 显示主窗（幂等：已可见就不再重复 show）。
fn reveal<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    if win.is_visible().unwrap_or(false) {
        return;
    }
    if let Err(e) = win.show() {
        log_warn!("显示主窗口失败: {e}");
        return;
    }
    let _ = win.set_focus();
}

/// 前端首帧就绪后调用：页面此时已按记忆皮肤渲染完，show 出来第一帧就是正确皮肤，
/// 不再露出 WebView2 在“show→页面加载完”之间的默认白底（真机表现为先闪一帧日间）。
pub fn show_main_now<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        reveal(&win);
    }
}

/// 前端首帧就绪信号（main.tsx 在 React 首次提交后调用）。前端异常时由兜底线程补位。
#[tauri::command]
pub fn main_window_ready(app: tauri::AppHandle) {
    show_main_now(&app);
}

/// 把记忆尺寸应用到（隐藏的）主窗、预先铺好首帧底色，**显示推迟到前端就绪**。
/// 由 `run()` 的 setup 调用。主窗以 `visible=false` 创建，因此既没有“先按默认尺寸
/// 露脸再变形”，也不会先露 WebView2 白底。
pub fn prepare_main_window(app: &tauri::AppHandle, settings: &Settings) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let (min, max) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .map(|w| {
            let min = (
                w.min_width.unwrap_or(0.0).round() as u32,
                w.min_height.unwrap_or(0.0).round() as u32,
            );
            let screen = win
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| win.primary_monitor().ok().flatten())
                .map(|m| {
                    let size = m.size();
                    (size.width, size.height)
                })
                .unwrap_or((u32::MAX, u32::MAX));
            (min, screen)
        })
        .unwrap_or(((0, 0), (u32::MAX, u32::MAX)));

    if let Some((w, h)) = initial_window_size(settings, min, max) {
        if let Err(e) = win.set_size(tauri::PhysicalSize::new(w, h)) {
            log_warn!("应用记忆窗口尺寸失败: {e}");
        }
    }
    // 首帧底色：仍保留作双保险（某些 WebView2 版本的第一帧早于页面绘制）
    let (r, g, b) = startup_background_rgb(&settings.theme);
    if let Err(e) = win.set_background_color(Some(tauri::window::Color(r, g, b, 255))) {
        log_warn!("设置首帧背景色失败: {e}");
    }
    // 兜底：前端若没发出 main_window_ready（脚本异常/加载失败），到点强制显示
    let app_fallback = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(SHOW_FALLBACK_MS));
        show_main_now(&app_fallback);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remembered(w: u32, h: u32) -> Settings {
        Settings {
            window_width: w,
            window_height: h,
            ..Settings::default()
        }
    }

    #[test]
    fn 无记忆尺寸不设启动尺寸() {
        assert_eq!(
            initial_window_size(&Settings::default(), (360, 540), (1920, 1080)),
            None,
            "缺省 0×0 应保持配置文件默认尺寸"
        );
    }

    #[test]
    fn 占位小值不设启动尺寸() {
        assert_eq!(
            initial_window_size(&remembered(1, 1), (360, 540), (1920, 1080)),
            None
        );
        assert_eq!(
            initial_window_size(&remembered(199, 800), (360, 540), (1920, 1080)),
            None,
            "单边是占位值也应拒绝"
        );
    }

    #[test]
    fn 正常记忆尺寸原样应用() {
        assert_eq!(
            initial_window_size(&remembered(600, 900), (360, 540), (1920, 1080)),
            Some((600, 900))
        );
    }

    #[test]
    fn 小于最小尺寸夹到最小() {
        assert_eq!(
            initial_window_size(&remembered(300, 500), (360, 540), (1920, 1080)),
            Some((360, 540))
        );
    }

    #[test]
    fn 超出屏幕夹到上限() {
        assert_eq!(
            initial_window_size(&remembered(4000, 3000), (360, 540), (1920, 1080)),
            Some((1920, 1080))
        );
    }

    #[test]
    fn 首帧底色跟随记忆皮肤() {
        // 夜间用户启动时窗口 show 到 WebView 加载完 index.html 之间会露出 WebView2 默认白底，
        // 表现为「先闪一帧日间」。底色必须与 index.html 内联脚本、前端 CSS 的 body 底色同值。
        assert_eq!(startup_background_rgb("dark"), (28, 27, 31));
        assert_eq!(startup_background_rgb("light"), (250, 248, 244));
        assert_eq!(startup_background_rgb(""), (250, 248, 244), "缺省/坏值回落日间底色");
    }
}
