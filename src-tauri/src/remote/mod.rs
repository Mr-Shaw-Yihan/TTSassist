// 移动端遥控（本体功能）：局域网 WebSocket 服务 + mDNS 发现 + 免码配对 + 自托管网页遥控。
//
// 原 lan-remote 插件（0.2.x）迁入本体，常驻启动，不再是可选插件：
// - server.rs   WS 服务与遥控协议（单端口 45271 分流 HTTP/WS，协议契约见设计文档 §三）
// - pairing.rs  配对记录 {token, device, paired_at, last_seen}（设备记忆增强）
// - mdns.rs     _ttsassist-remote._tcp 局域网发现广播
// - web.rs      自托管网页遥控器（remote_page.html 编译进本体）
//
// 对外：spawn(app, data_dir) 启动服务；RemoteCore 作为 Tauri State 供 remote_session_info 读取；
// purge_legacy_plugin 在插件加载前清理旧 lan-remote（迁移 token + 删注册表条目/目录/内置 zip）。
// 宿主能力经 plugins::bridge 的 native_* 函数直接调用（不再走 C ABI）。

pub mod mdns;
pub mod pairing;
pub mod server;
pub mod web;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

pub use server::{Shared, PORT};

/// 会话信息（前端遥控页实时展示）：已配对设备 + 当前连接态。
/// 由 server 在连接/断开/配对/顶替时经 "remote:status" 事件推送，也可命令拉取。
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SessionInfo {
    /// 是否已配对（存在有效 token）
    pub paired: bool,
    /// 已配对设备名（旧 token 迁移无设备名时为 null）
    pub device: Option<String>,
    /// 配对时间（ISO8601）
    pub paired_at: Option<String>,
    /// 最近连接时间（ISO8601）
    pub last_seen: Option<String>,
    /// 当前是否有已鉴权连接
    pub connected: bool,
    /// 当前连接对端地址（ip:port；未连接为 null）
    pub peer: Option<String>,
}

/// 遥控核心（Tauri State）：持有 WS 服务共享态，供命令读取会话信息。
pub struct RemoteCore {
    shared: Arc<Shared>,
}

impl RemoteCore {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// 当前会话信息快照
    pub fn session_info(&self) -> SessionInfo {
        self.shared.session_info()
    }
}

/// 查询遥控会话信息（前端遥控页挂载时拉取；实时变化另经 remote:status 事件推送）
#[tauri::command]
pub fn remote_session_info(core: tauri::State<'_, RemoteCore>) -> SessionInfo {
    core.session_info()
}

/// 启动本体遥控服务：在 tauri::async_runtime（底层即 tokio 多线程运行时）上跑
/// WS 服务，另起独立线程做 mDNS 广播。返回共享态（供 manage 成 RemoteCore）。
///
/// 须在 HostBridge / AppState / MicPlayback 就绪后调用（native_* 能力依赖它们）。
pub fn spawn(app: AppHandle, data_dir: PathBuf) -> Arc<Shared> {
    // 配对记录：加载持久化（旧 token 已由 purge_legacy_plugin 迁移到本体数据目录）
    let pairing = pairing::Pairing::load(&data_dir);
    let shared = Arc::new(server::Shared::new(pairing, app.clone()));

    // 事件转发：注册到宿主能力桥，与插件共用同一套事件 JSON
    // （favorites_changed / settings_changed / playback_changed / state_changed）
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(bridge) = app.try_state::<crate::plugins::bridge::HostBridge>() {
        bridge.register_native_event_sink(event_tx);
    } else {
        log_warn!("[remote] 宿主能力桥未就绪，状态推送不可用");
    }

    // WS 服务：监听局域网（0.0.0.0），端口被占则 5 秒重试不退出
    let shared_run = Arc::clone(&shared);
    tauri::async_runtime::spawn(async move {
        let listener = loop {
            match tokio::net::TcpListener::bind(("0.0.0.0", server::PORT)).await {
                Ok(l) => break l,
                Err(e) => {
                    log_error!("[remote] 端口 {} 监听失败（{e}），5 秒后重试", server::PORT);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        };
        log_info!("[remote] WS 服务已启动: 0.0.0.0:{}", server::PORT);
        server::run(listener, shared_run, event_rx).await;
    });

    // mDNS 广播（独立线程，探测失败自动跳过；App 仍可手动填 IP 连接）
    mdns::spawn_broadcast(server::PORT);

    shared
}

/// 清理旧 lan-remote 插件（须在 PluginManager::load_all 之前调用）：
/// 1. 迁移旧 token（本体记录不存在且旧插件 token.json 存在 → 老用户免重配）；
/// 2. 从注册表移除 lan-remote 条目并删除插件目录；
/// 3. 删除残留内置 zip（防止 bootstrap_bundled_zips 重装抢占 45271）。
pub fn purge_legacy_plugin(plugins_root: &Path, data_dir: &Path) {
    const LEGACY_ID: &str = "lan-remote";
    let legacy_dir = plugins_root.join(LEGACY_ID);

    // 1. 迁移旧 token（旧格式仅 {token}，迁为无设备名的本体记录）
    let new_path = data_dir.join(pairing::FILE);
    let old_token = legacy_dir.join("data").join("token.json");
    if !new_path.exists() && old_token.exists() {
        if let Ok(raw) = std::fs::read_to_string(&old_token) {
            let raw = raw.trim_start_matches('\u{FEFF}');
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                if let Some(token) = v
                    .get("token")
                    .and_then(|t| t.as_str())
                    .filter(|s| !s.is_empty())
                {
                    let record = pairing::PairRecord {
                        token: token.to_string(),
                        device: None,
                        paired_at: None,
                        last_seen: None,
                    };
                    let _ = std::fs::create_dir_all(data_dir);
                    match crate::storage::atomic::write_json_pretty(&new_path, &record) {
                        Ok(()) => log_info!("[remote] 已迁移旧 lan-remote 配对 token（老用户免重配）"),
                        Err(e) => log_error!("[remote] 迁移旧配对 token 失败: {e}"),
                    }
                }
            }
        }
    }

    // 2. 注册表移除条目 + 删插件目录（否则 load_all 会重新加载它，与本体服务抢 45271）
    let mut reg = crate::plugins::registry::load_registry(plugins_root);
    let before = reg.plugins.len();
    reg.plugins.retain(|e| e.id != LEGACY_ID);
    if reg.plugins.len() != before {
        match crate::plugins::registry::save_registry(plugins_root, &reg) {
            Ok(()) => log_info!("[remote] 已从注册表移除 lan-remote 条目"),
            Err(e) => log_error!("[remote] 移除 lan-remote 注册表条目失败: {e}"),
        }
    }
    if legacy_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&legacy_dir) {
            log_error!("[remote] 删除旧 lan-remote 插件目录失败: {e}");
        }
    }

    // 3. 删除残留内置 zip（与 PluginManager::load_all 相同的候选层级）
    for dir in bundled_zip_dirs() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if p.is_file() && name.starts_with("lan-remote-") && name.ends_with(".zip") {
                match std::fs::remove_file(&p) {
                    Ok(()) => log_info!("[remote] 已删除残留内置 zip: {}", p.display()),
                    Err(e) => log_error!("[remote] 删除残留内置 zip 失败 {}: {e}", p.display()),
                }
            }
        }
    }
}

/// 内置 zip 候选目录（Windows 上 resource_dir = exe 根；与 PluginManager::load_all 一致）：
/// <exe>/plugins 与 <exe>/resources/plugins 两级。
fn bundled_zip_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent() {
            dirs.push(root.join("plugins"));
            dirs.push(root.join("resources").join("plugins"));
        }
    }
    dirs
}
