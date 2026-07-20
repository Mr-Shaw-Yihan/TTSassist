// 系统托盘：图标 + 菜单（显示主窗 / 退出）
// 关闭主窗 = 最小化到托盘不退出；托盘"显示主窗"= show+focus；托盘"退出"= 真正退出。

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};

/// 初始化系统托盘。在 setup 里调用一次。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    // 菜单项
    let show = MenuItem::with_id(app, "tray_show", "显示主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray_quit", "退出 VoiceAssist", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("缺少默认窗口图标").clone())
        .tooltip("VoiceAssist · 语笺")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "tray_quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// 给主窗挂"关闭=最小化到托盘"逻辑。在 setup 后用窗口事件处理。
pub fn install_close_to_tray(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let app_handle = app.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 拦截关闭，改为隐藏
                api.prevent_close();
                let _ = app_handle.get_webview_window("main").map(|w| w.hide());
            }
        });
    }
}