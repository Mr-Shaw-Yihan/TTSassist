// 全局快捷键后端：Windows 低级键盘钩子（WH_KEYBOARD_LL）。
//
// 为什么不用 RegisterHotKey（tauri-plugin-global-shortcut 底层）：
// 全屏独占游戏（如英雄联盟）会独占键盘输入，RegisterHotKey 注册的快捷键无法触发。
// WH_KEYBOARD_LL 是用户态低级钩子——键盘事件在系统层先送达钩子回调再分发给前台窗口，
// 不挑前台窗口；且它不是注入型钩子：回调只运行在本进程的钩子线程中，不会进入任何
// 其它（游戏）进程，竞技游戏反作弊不会将其视为外挂（NVIDIA App 呼出浮窗即同类机制）。
//
// 线程模型：
// - hook 线程：安装 WH_KEYBOARD_LL + 消息泵（LL 钩子回调依赖线程消息循环投递）；
//   钩子回调内只做「查表匹配 + 通道投递」的轻量工作（系统对 LL 钩子响应有严格超时）。
// - worker 线程：串行执行实际业务回调（窗口显隐、emit 等），避免阻塞钩子线程。
//
// 匹配语义与 RegisterHotKey 对齐：修饰键精确匹配（Ctrl+Alt+V 不会误配 Ctrl+V），
// 命中后吞掉按键（不转发给前台窗口）；按住主键的自动重复只触发一次 Pressed。

use std::sync::Arc;

/// 快捷键阶段：按下 / 松开（语音输入「按住说话」两者都需要）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HotkeyPhase {
    Pressed,
    Released,
}

pub type HotkeyCallback = Arc<dyn Fn(HotkeyPhase) + Send + Sync>;

#[cfg(windows)]
mod win {
    use super::{HotkeyCallback, HotkeyPhase};
    use std::collections::HashMap;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::mpsc;
    use std::sync::{Arc, LazyLock, Mutex, Once, OnceLock};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG,
    };

    // ── Win32 常量（避免依赖额外 feature） ──
    const WH_KEYBOARD_LL: i32 = 13;
    const HC_ACTION: i32 = 0;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_KEYUP: u32 = 0x0101;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSKEYUP: u32 = 0x0105;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_MENU: u16 = 0x12;
    const VK_LWIN: u16 = 0x5B;
    const VK_RWIN: u16 = 0x5C;

    // ── 修饰键位标记 ──
    const MOD_CTRL: u8 = 0b0001;
    const MOD_ALT: u8 = 0b0010;
    const MOD_SHIFT: u8 = 0b0100;
    const MOD_WIN: u8 = 0b1000;

    struct Binding {
        mods: u8,
        vk: u16,
        cb: HotkeyCallback,
    }

    type Job = Box<dyn FnOnce() + Send + 'static>;

    /// accel 串 → 绑定表（同一 accel 全局唯一，由 hotkey.rs 的冲突检测保证）
    static BINDINGS: LazyLock<Mutex<HashMap<String, Binding>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    /// 注册快捷键。accel 为前端产生的加速键串（如 "Ctrl+Alt+V"）。
    /// 幂等覆盖：同一 accel 重复注册时替换回调。
    pub fn register(accel: &str, cb: impl Fn(HotkeyPhase) + Send + Sync + 'static) -> Result<(), String> {
        let (mods, vk) = parse_accel(accel)?;
        init_threads();
        let mut table = BINDINGS.lock().map_err(|e| format!("锁失败：{e}"))?;
        table.insert(accel.to_string(), Binding { mods, vk, cb: Arc::new(cb) });
        Ok(())
    }

    /// 注销快捷键（幂等）
    pub fn unregister(accel: &str) {
        if let Ok(mut table) = BINDINGS.lock() {
            table.remove(accel);
        }
    }

    /// 安装 hook 线程 + worker 线程（进程内一次）
    fn init_threads() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let _ = DISPATCH_TX.set(tx);
            // worker：串行执行业务回调，单个回调 panic 不杀死线程
            let _ = std::thread::Builder::new()
                .name("hotkey-worker".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                });
            // hook：安装 LL 键盘钩子并泵消息
            let _ = std::thread::Builder::new()
                .name("hotkey-hook".into())
                .spawn(hook_thread);
        });
    }

    fn hook_thread() {
        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                eprintln!("安装低级键盘钩子失败, error={}", GetLastError());
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            // GetMessage 返回 0（WM_QUIT）或 -1（错误）时退出；LL 钩子回调依赖此消息泵投递
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&mut msg);
            }
            UnhookWindowsHookEx(hook);
        }
    }

    /// 钩子命中后把业务回调投递给 worker 线程执行
    static DISPATCH_TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

    // 钩子线程上「已触发 Pressed 的主键」表：vk → 回调（松键时精确找回同一绑定）
    thread_local! {
        static PRESSED: std::cell::RefCell<HashMap<u16, HotkeyCallback>> = std::cell::RefCell::new(HashMap::new());
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
        if code != HC_ACTION || lparam == 0 {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }
        let msg = wparam as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if !is_down && !is_up {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }

        let data = &*(lparam as *const KBDLLHOOKSTRUCT);
        let vk = data.vkCode as u16;

        // 修饰键本身不做主键匹配，直接放行
        if matches!(vk, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN) {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }

        let table = match BINDINGS.lock() {
            Ok(t) => t,
            Err(_) => return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam),
        };

        if is_down {
            let already = PRESSED.with_borrow(|p| p.contains_key(&vk));
            if already {
                // 自动重复：不重复触发，但仍吞键（与首次按下行为一致）
                if table.values().any(|b| b.vk == vk) {
                    return 1;
                }
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }
            // 修饰键精确匹配，避免 Ctrl+Alt+V 误配 Ctrl+V
            if let Some(b) = table.values().find(|b| b.vk == vk && mods_state_matches(b.mods)) {
                let cb = b.cb.clone();
                drop(table);
                PRESSED.with_borrow_mut(|p| {
                    p.insert(vk, cb.clone());
                });
                dispatch(cb, HotkeyPhase::Pressed);
                return 1; // 吞键：前台窗口不再收到该组合键
            }
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }

        // 松键：只对「曾触发过 Pressed」的键派发 Released 并吞键
        if let Some(cb) = PRESSED.with_borrow_mut(|p| p.remove(&vk)) {
            dispatch(cb, HotkeyPhase::Released);
            return 1;
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    /// 当前物理修饰键状态是否与绑定要求完全一致
    fn mods_state_matches(required: u8) -> bool {
        unsafe {
            let ctrl = GetAsyncKeyState(VK_CONTROL as i32) < 0;
            let alt = GetAsyncKeyState(VK_MENU as i32) < 0;
            let shift = GetAsyncKeyState(VK_SHIFT as i32) < 0;
            let win = GetAsyncKeyState(VK_LWIN as i32) < 0 || GetAsyncKeyState(VK_RWIN as i32) < 0;
            ctrl == (required & MOD_CTRL != 0)
                && alt == (required & MOD_ALT != 0)
                && shift == (required & MOD_SHIFT != 0)
                && win == (required & MOD_WIN != 0)
        }
    }

    fn dispatch(cb: HotkeyCallback, phase: HotkeyPhase) {
        if let Some(tx) = DISPATCH_TX.get() {
            let _ = tx.send(Box::new(move || cb(phase)));
        }
    }

    /// 解析前端加速键串（HotkeyRecorder/accelerator.ts 产生）为 (修饰键位标记, VK 码)。
    /// 例："Ctrl+Alt+V" → (MOD_CTRL|MOD_ALT, 0x56)
    fn parse_accel(accel: &str) -> Result<(u8, u16), String> {
        let mut mods = 0u8;
        let mut vk: Option<u16> = None;
        for part in accel.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= MOD_CTRL,
                "alt" => mods |= MOD_ALT,
                "shift" => mods |= MOD_SHIFT,
                "meta" | "win" | "super" => mods |= MOD_WIN,
                _ => {
                    if vk.is_some() {
                        return Err(format!("快捷键 `{accel}` 含多个主键"));
                    }
                    vk = Some(
                        key_to_vk(part)
                            .ok_or_else(|| format!("快捷键中的按键 `{part}` 无法识别"))?,
                    );
                }
            }
        }
        let vk = vk.ok_or_else(|| "快捷键缺少主键".to_string())?;
        Ok((mods, vk))
    }

    /// 按键名 → Windows 虚拟键码（覆盖 accelerator.ts mapKey 的全部产出）
    fn key_to_vk(key: &str) -> Option<u16> {
        if key.len() == 1 {
            return match key.chars().next().unwrap().to_ascii_uppercase() {
                c @ 'A'..='Z' => Some(0x41 + (c as u16 - 'A' as u16)),
                c @ '0'..='9' => Some(0x30 + (c as u16 - '0' as u16)),
                _ => None,
            };
        }
        match key.to_ascii_lowercase().as_str() {
            "space" => return Some(0x20),
            "up" => return Some(0x26),
            "down" => return Some(0x28),
            "left" => return Some(0x25),
            "right" => return Some(0x27),
            "escape" | "esc" => return Some(0x1B),
            "enter" | "return" => return Some(0x0D),
            "tab" => return Some(0x09),
            "backspace" => return Some(0x08),
            "delete" | "del" => return Some(0x2E),
            "insert" => return Some(0x2D),
            "home" => return Some(0x24),
            "end" => return Some(0x23),
            "pageup" => return Some(0x21),
            "pagedown" => return Some(0x22),
            _ => {}
        }
        // F1 ~ F24
        if let Some(n) = key.to_ascii_lowercase().strip_prefix('f') {
            if let Ok(num) = n.parse::<u16>() {
                if (1..=24).contains(&num) {
                    return Some(0x6F + num); // F1 = 0x70
                }
            }
        }
        None
    }
}

#[cfg(windows)]
pub use win::{register, unregister};

#[cfg(not(windows))]
pub fn register(_accel: &str, _cb: impl Fn(HotkeyPhase) + Send + Sync + 'static) -> Result<(), String> {
    Err("低级键盘钩子仅支持 Windows".into())
}

#[cfg(not(windows))]
pub fn unregister(_accel: &str) {}
