// 游戏场景窗口焦点控制（裸 FFI，不引第三方库）。
//
// 背景：独占全屏游戏被抢焦点会最小化，无边框游戏被抢焦点会暂停。
// - show_no_activate：浮窗呼出瞬间不抢前台焦点（ShowWindow SW_SHOWNOACTIVATE）
// - set_no_activate：给悬浮球挂 WS_EX_NOACTIVATE（点击不激活、不抢焦点；
//   config 的 focus:false 只管创建时机，点击后仍会激活球窗）
// - foreground_info：焦点诊断日志用，抓失焦瞬间的前台窗口身份
// - focus_webview_child：直播伴侣形态——键盘焦点给 webview 子窗口，
//   前台窗口不变（打字/ESC 可用但游戏不失焦；激活与键盘焦点是两回事）


use tauri::WebviewWindow;

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(h_wnd: isize, n_cmd_show: i32) -> i32;
    fn GetWindowLongPtrW(h_wnd: isize, n_index: i32) -> isize;
    fn SetWindowLongPtrW(h_wnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    fn GetForegroundWindow() -> isize;
    fn GetWindowTextW(h_wnd: isize, lp_string: *mut u16, n_max_count: i32) -> i32;
    fn GetWindow(h_wnd: isize, u_cmd: u32) -> isize;
    fn GetClassNameW(h_wnd: isize, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    fn SetFocus(h_wnd: isize) -> isize;
    fn MessageBoxW(
        h_wnd: isize,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;
}

const MB_YESNO: u32 = 0x0004;
const MB_ICONQUESTION: u32 = 0x0020;
const MB_SYSTEMMODAL: u32 = 0x1000; // 系统级置顶：宿主在后台时也保证可见
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const IDYES: i32 = 6;
const IDNO: i32 = 7;

const SW_SHOWNOACTIVATE: i32 = 4;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_NOACTIVATE: isize = 0x0800_0000;
const GW_CHILD: u32 = 5;
const GW_HWNDNEXT: u32 = 2;

/// 无激活显示窗口（不抢前台焦点）。返回是否调用成功。
pub fn show_no_activate(win: &WebviewWindow) -> bool {
    match win.hwnd() {
        Ok(h) => unsafe { ShowWindow(h.0 as isize, SW_SHOWNOACTIVATE) != 0 },
        Err(_) => false,
    }
}

/// 设置/取消窗口的 WS_EX_NOACTIVATE（鼠标点击不激活窗口、不抢焦点；鼠标事件本身不受影响）
pub fn set_no_activate(win: &WebviewWindow, on: bool) {
    if let Ok(h) = win.hwnd() {
        unsafe {
            let hwnd = h.0 as isize;
            let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let new = if on { cur | WS_EX_NOACTIVATE } else { cur & !WS_EX_NOACTIVATE };
            if new != cur {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
            }
        }
    }
}

/// 系统置顶是/否弹窗（免码配对等「必须被看到」的确认场景）。
/// 返回 Some(true)=是，Some(false)=否，None=异常。阻塞调用，调用方应放后台线程；
/// MessageBoxW 原生支持任意线程调用。
pub fn confirm_yes_no(title: &str, body: &str) -> Option<bool> {
    let t: Vec<u16> = title.encode_utf16().chain([0]).collect();
    let b: Vec<u16> = body.encode_utf16().chain([0]).collect();
    let r = unsafe {
        MessageBoxW(
            0,
            b.as_ptr(),
            t.as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_SYSTEMMODAL | MB_SETFOREGROUND,
        )
    };
    match r {
        IDYES => Some(true),
        IDNO => Some(false),
        _ => None,
    }
}

/// 把键盘焦点交给窗口内的 WebView2 子窗口（不激活窗口、前台不变）。
/// 直播伴侣形态的关键：WS_EX_NOACTIVATE 窗口点击后没有激活事件，
/// 键盘路由靠 SetFocus 建立——wry 的 WebView2 是宿主下的子窗口（类名
/// Chrome_WidgetWin_*），优先选它，选不到退回第一个子窗口。
/// 必须从拥有窗口的主线程调用（tauri 同步 command 即主线程）。
pub fn focus_webview_child(win: &WebviewWindow) -> bool {
    let Ok(h) = win.hwnd() else { return false };
    let host = h.0 as isize;
    unsafe {
        let mut target = 0isize;
        let mut fallback = 0isize;
        let mut child = GetWindow(host, GW_CHILD);
        while child != 0 {
            if fallback == 0 {
                fallback = child;
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(child, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 {
                let cls = String::from_utf16_lossy(&buf[..n as usize]);
                if cls.starts_with("Chrome_WidgetWin") {
                    target = child;
                    break;
                }
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
        let t = if target != 0 { target } else { fallback };
        t != 0 && SetFocus(t) != 0
    }
}

/// 探测当前前台窗口 (hwnd, 窗口标题)，用于焦点诊断
pub fn foreground_info() -> (isize, String) {
    unsafe {
        let h = GetForegroundWindow();
        if h == 0 {
            return (0, String::new());
        }
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        let n = n.clamp(0, buf.len() as i32) as usize;
        (h, String::from_utf16_lossy(&buf[..n]))
    }
}
