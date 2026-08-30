// lan-remote 插件：手机遥控 PC 端（局域网 WebSocket 服务）。
//
// 组成：
// - lib.rs      C ABI 导出 + va_plugin_attach_host（宿主能力桥接入点）
// - server.rs   WS 服务器与遥控协议（协议契约见 doc/移动端遥控器设计.md §三）
// - pairing.rs  6 位配对码 / token 管理（一对一，新配对顶替旧会话）
// - mdns_adv.rs _ttsassist-remote._tcp 局域网发现广播
//
// 生命周期：宿主注入能力桥（attach）即启动 WS 服务与 mDNS；
// dll 常驻进程，卸载=重启宿主后彻底消失（宿主插件系统约束）。

mod mdns_adv;
mod pairing;
mod server;
mod web;

use std::sync::OnceLock;
use tokio::sync::mpsc;

// ── C ABI 导出（服务插件最小集：id/name/version + 能力桥接入）──

// &str 字面量可含 UTF-8 与 NUL（b"" 字节串不支持中文）
static PLUGIN_ID: &str = "lan-remote\0";
static PLUGIN_NAME: &str = "手机遥控（局域网）\0";
static PLUGIN_VERSION: &str = "0.1.0\0";

#[no_mangle]
pub extern "C" fn va_plugin_id() -> *const std::os::raw::c_char {
    PLUGIN_ID.as_ptr() as *const std::os::raw::c_char
}

#[no_mangle]
pub extern "C" fn va_plugin_name() -> *const std::os::raw::c_char {
    PLUGIN_NAME.as_ptr() as *const std::os::raw::c_char
}

#[no_mangle]
pub extern "C" fn va_plugin_version() -> *const std::os::raw::c_char {
    PLUGIN_VERSION.as_ptr() as *const std::os::raw::c_char
}

// 宿主能力桥接入点（导出 va_plugin_attach_host：存表 → on_attach）
plugin_api::va_host_bridge! {
    on_attach: on_attach,
}

// ── 插件内部状态 ──────────────────────────────────────

/// 宿主事件 → WS 服务器的转发通道（宿主回调线程安全发送）
static EVENT_TX: OnceLock<mpsc::UnboundedSender<String>> = OnceLock::new();

/// 插件自有 tokio 运行时（与宿主运行时隔离；持有即存活）
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// 宿主能力桥接入：存表后启动全部服务（进程内只启动一次）
fn on_attach(_services: &plugin_api::VaHostServices) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("lan-remote")
        .enable_all()
        .build()
        .expect("[lan-remote] 无法创建 tokio 运行时");
    let rt = RUNTIME.get_or_init(|| rt);

    let (event_tx, event_rx) = mpsc::unbounded_channel::<String>();
    let _ = EVENT_TX.set(event_tx);

    // 配对状态：加载持久化 token（配对码路径已移除，唯一配对路径为免码弹窗确认）
    let pairing = pairing::Pairing::load();

    let shared = std::sync::Arc::new(server::Shared::new(pairing));

    // 订阅宿主事件（收藏/设置/播放状态变化）→ 转发给 WS 服务器
    if let Err(e) = plugin_api::host_bridge::subscribe_events(|event_json| {
        if let Some(tx) = EVENT_TX.get() {
            let _ = tx.send(event_json.to_string());
        }
    }) {
        eprintln!("[lan-remote] 订阅宿主事件失败（状态推送不可用）: {e}");
    }

    // WS 服务器：监听局域网（0.0.0.0），失败重试不退出
    rt.spawn(async move {
        loop {
            match tokio::net::TcpListener::bind(("0.0.0.0", server::PORT)).await {
                Ok(listener) => {
                    eprintln!("[lan-remote] WS 服务已启动: 0.0.0.0:{}", server::PORT);
                    server::run(listener, shared, event_rx).await;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[lan-remote] 端口 {} 监听失败（{}），5 秒后重试",
                        server::PORT, e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    // mDNS 广播（独立线程，探测失败自动跳过）
    mdns_adv::spawn_broadcast(server::PORT);

    // 遥控地址上屏（阻塞 FFI 放独立线程，attach 尽快返回）
    let host_addr = mdns_adv::local_lan_ip()
        .map(|ip| format!("{ip}:{}", server::PORT))
        .unwrap_or_else(|| format!("127.0.0.1:{}", server::PORT));
    std::thread::spawn(move || {
        if let Err(e) = plugin_api::host_bridge::set_own_config("host_addr", &host_addr) {
            eprintln!("[lan-remote] 遥控地址上屏失败: {e}");
        }
    });
}
