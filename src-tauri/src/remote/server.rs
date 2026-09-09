// WebSocket 服务器与遥控协议（协议契约见 doc/移动端遥控器设计.md §三）。
//
// 原 lan-remote 插件 server.rs 迁入本体：去掉 C ABI，宿主能力经 plugins::bridge
// 的 native_* 函数直接调用；连接/断开/配对/顶替时 emit "remote:status" 供前端遥控页实时反映。
//
// 模型：
// - 一对一：同一时刻只有一个已鉴权连接，新配对/新 hello 顶替旧会话；
// - 未鉴权连接只允许 pair_request / hello / ping，其余回 error 并断开
//   （配对码路径 pair / refresh_code 已随 1.8.x 移除，收到即回错误并断开）；
// - 宿主事件（event_rx，来自 HostBridge 广播）转发给已鉴权连接：先透传 event，再重查 state 推送。

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message;

use super::pairing::Pairing;
use super::SessionInfo;
use crate::plugins::bridge::{
    native_list_favorites, native_play_last, native_stop_playback, native_synthesize,
    native_toggle_mic, play_favorite_by_id, state_json,
};

/// HTTP/WS 同端口分流：把已读首包作为前缀"塞回"流，
/// 让 tungstenite 的握手从完整请求开始（TcpStream 不支持 unpeek）。
struct PrefixedIo {
    prefix: std::io::Cursor<Vec<u8>>,
    inner: TcpStream,
}

impl PrefixedIo {
    fn new(first: Vec<u8>, inner: TcpStream) -> Self {
        Self { prefix: std::io::Cursor::new(first), inner }
    }
}

impl AsyncRead for PrefixedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // 先耗尽前缀（首包），再透传底层流
        let prefix_len = self.prefix.get_ref().len() as u64;
        if self.prefix.position() < prefix_len {
            let pos = self.prefix.position() as usize;
            let remaining = &self.prefix.get_ref()[pos..];
            let take = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..take]);
            self.prefix.set_position((pos + take) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PrefixedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Web 遥控页面 HTTP 响应（单文件，随请求即答即关）
async fn serve_http(stream: &mut TcpStream, first: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(first);
    let path = text.split_whitespace().nth(1).unwrap_or("/");
    let (status, ctype, body) = super::web::http_response(path);
    log_info!("[remote] Web 遥控页面请求: {path} → {status}");
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|e| format!("写 HTTP 响应失败: {e}"))?;
    stream
        .write_all(&body)
        .await
        .map_err(|e| format!("写 HTTP 响应失败: {e}"))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("刷 HTTP 响应失败: {e}"))?;
    let _ = stream.shutdown().await;
    Ok(())
}

/// WS 监听端口（协议契约：App 侧 mDNS 发现失败时手动填 ip:45271 兜底）
pub const PORT: u16 = 45271;

/// 当前已鉴权连接的句柄（顶替旧会话用）
struct AuthedConn {
    out: mpsc::UnboundedSender<String>,
    kill: Arc<Notify>,
    /// 连接对端地址（ip:port），供遥控页展示
    peer: String,
}

/// 全局共享状态
pub struct Shared {
    app: AppHandle,
    authed: Mutex<Option<AuthedConn>>,
    pub pairing: Mutex<Pairing>,
}

impl Shared {
    pub fn new(pairing: Pairing, app: AppHandle) -> Self {
        Self {
            app,
            authed: Mutex::new(None),
            pairing: Mutex::new(pairing),
        }
    }

    /// 会话信息快照（供 remote_session_info 命令 + remote:status 事件）：
    /// 已配对设备（名/配对时间/最近连接）+ 当前是否有鉴权连接（对端地址）
    pub fn session_info(&self) -> SessionInfo {
        let (connected, peer) = {
            let guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_ref() {
                Some(c) => (true, Some(c.peer.clone())),
                None => (false, None),
            }
        };
        let p = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        SessionInfo {
            paired: p.is_paired(),
            device: p.device(),
            paired_at: p.paired_at(),
            last_seen: p.last_seen(),
            connected,
            peer,
        }
    }

    /// 广播会话状态到前端遥控页（连接/断开/配对/顶替时调用）
    fn emit_status(&self) {
        let _ = self.app.emit("remote:status", self.session_info());
    }

    /// 注册新的已鉴权连接，顶替并关闭旧连接（一对一）。
    /// 顶替前先给旧连接排一条 error 告知「已被新设备接管」，再唤醒其关闭
    /// （旧连接在关闭前会把已排队消息发出，见 handle_conn 的 taken_over 分支）。
    fn take_over(
        &self,
        out: mpsc::UnboundedSender<String>,
        kill: Arc<Notify>,
        peer: String,
    ) {
        let old = {
            let mut guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
            guard.replace(AuthedConn { out, kill, peer })
        };
        if let Some(old) = old {
            let _ = old.out.send(err_msg("已被新设备接管"));
            old.kill.notify_waiters();
            log_info!("[remote] 新会话已连接，旧会话被顶替");
        }
        self.emit_status();
    }

    /// 给已鉴权连接发一条 JSON（无连接时丢弃）
    fn send_authed(&self, json: String) {
        let guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = guard.as_ref() {
            let _ = conn.out.send(json);
        }
    }

    /// 连接断开后清理（只清理仍指向自己的句柄），并广播状态
    fn clear_authed(&self, out: &mpsc::UnboundedSender<String>) {
        let is_current = {
            let mut guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
            let is_current = guard.as_ref().map(|c| c.out.same_channel(out)) == Some(true);
            if is_current {
                *guard = None;
            }
            is_current
        };
        if is_current {
            self.emit_status();
        }
    }
}

/// 服务器主循环：接受连接 + 转发宿主事件。占用当前任务直至进程退出。
pub async fn run(
    listener: TcpListener,
    shared: Arc<Shared>,
    mut event_rx: mpsc::UnboundedReceiver<String>,
) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, addr)) => {
                        let shared = Arc::clone(&shared);
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, shared).await {
                                log_warn!("[remote] 连接 {addr} 结束: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        log_error!("[remote] 接受连接失败: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            // 宿主事件 → 已鉴权连接：透传 event + 重查 state 推送
            Some(event) = event_rx.recv() => {
                let evt_msg = serde_json::json!({
                    "t": "event",
                    "event": serde_json::from_str::<serde_json::Value>(&event)
                        .unwrap_or(serde_json::Value::Null),
                });
                shared.send_authed(evt_msg.to_string());
                // 重查状态并推送（读盘，放后台线程）
                let app = shared.app.clone();
                let state = tokio::task::spawn_blocking(move || state_json(&app))
                    .await
                    .unwrap_or_default();
                if !state.is_empty() {
                    shared.send_authed(serde_json::json!({
                        "t": "state",
                        "state": serde_json::from_str::<serde_json::Value>(&state)
                            .unwrap_or(serde_json::Value::Null),
                    }).to_string());
                }
            }
            else => break,
        }
    }
}

/// 单连接处理：读首包分流 → HTTP（Web 遥控页面）或 WS（遥控协议）
async fn handle_conn(mut stream: TcpStream, shared: Arc<Shared>) -> Result<(), String> {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".into());
    // 读首包（请求头一般 <8KB；WS upgrade 与普通 HTTP GET 都完整落在首包内）
    let mut first = vec![0u8; 8192];
    let n = stream
        .read(&mut first)
        .await
        .map_err(|e| format!("读取首包失败（{peer}）: {e}"))?;
    if n == 0 {
        return Ok(());
    }
    first.truncate(n);
    let is_ws = first
        .windows(18)
        .any(|w| w.eq_ignore_ascii_case(b"upgrade: websocket"));

    if !is_ws {
        // HTTP：Web 遥控页面（与 WS 同源，浏览器无 Mixed Content 限制）
        serve_http(&mut stream, &first).await?;
        return Ok(());
    }

    // WS：首包需要"塞回"流里交给 tungstenite 握手 → PrefixedIo 前缀包装
    let prefixed = PrefixedIo::new(first, stream);
    let ws = tokio_tungstenite::accept_async(prefixed)
        .await
        .map_err(|e| format!("WS 握手失败（{peer}）: {e}"))?;
    log_info!("[remote] WS 握手成功（{peer}）");
    let (mut sink, mut source) = ws.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let kill = Arc::new(Notify::new());
    let mut authenticated = false;
    let mut closed = false;
    let mut taken_over = false;

    while !closed {
        tokio::select! {
            // 出站队列（命令回执 / 状态推送）
            out = out_rx.recv() => {
                match out {
                    Some(json) => {
                        if sink.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            // 新会话顶替：标记后退出循环，关闭前先flush已排队的告知消息
            _ = kill.notified() => {
                taken_over = true;
                break;
            }
            msg = source.next() => {
                let Some(Ok(msg)) = msg else { break };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Ping(_) | Message::Pong(_) => continue, // 协议层心跳自动处理
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Frame(_) => {
                        let _ = sink.send(Message::Text(err_msg("仅支持 JSON 文本消息"))).await;
                        continue;
                    }
                };
                let action =
                    handle_message(&text, &shared, &out_tx, &kill, &peer, &mut authenticated).await;
                match action {
                    Action::Continue => {}
                    Action::CloseNow => {
                        let _ = sink.send(Message::Close(None)).await;
                        closed = true;
                    }
                }
            }
        }
    }

    // 被顶替：关闭前把已排队的告知消息（"已被新设备接管"）发出，再关连接
    if taken_over {
        while let Ok(json) = out_rx.try_recv() {
            if sink.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    }

    if authenticated {
        shared.clear_authed(&out_tx);
        log_info!("[remote] 已鉴权连接断开（{peer}）");
    }
    Ok(())
}

enum Action {
    Continue,
    CloseNow,
}

fn err_msg(err: &str) -> String {
    serde_json::json!({ "t": "error", "err": err }).to_string()
}

/// 解析并处理一条 c2s 消息（协议见设计文档 §三）。
/// kill：本连接的顶替通知句柄（鉴权成功注册到全局，新会话到来时被唤醒关闭）
async fn handle_message(
    text: &str,
    shared: &Arc<Shared>,
    out: &mpsc::UnboundedSender<String>,
    kill: &Arc<Notify>,
    peer: &str,
    authenticated: &mut bool,
) -> Action {
    let msg: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            let _ = out.send(err_msg("消息不是合法 JSON"));
            return Action::Continue;
        }
    };
    let t = msg.get("t").and_then(|v| v.as_str()).unwrap_or("");
    let ref_id = msg.get("ref").and_then(|v| v.as_str()).map(str::to_string);

    match t {
        // ── 鉴权前允许 ──
        // 配对码路径（pair / refresh_code）已随 1.8.x 移除：收到即回错误并断开
        "pair" | "refresh_code" => {
            let _ = out.send(err_msg("配对码已停用，请在 App 使用自动发现或手动连接后弹窗配对"));
            return Action::CloseNow;
        }
        "hello" => {
            let token = msg.get("token").and_then(|v| v.as_str()).unwrap_or("");
            // 命中即刷新最近连接时间（设备记忆）
            let ok = {
                let mut pairing = shared.pairing.lock().unwrap_or_else(|e| e.into_inner());
                let ok = pairing.check_token(token);
                if ok {
                    pairing.touch();
                }
                ok
            };
            if ok {
                let state = current_state(&shared.app).await;
                let resp = serde_json::json!({ "t": "hello_ok", "state": state });
                let _ = out.send(resp.to_string());
                *authenticated = true;
                shared.take_over(out.clone(), Arc::clone(kill), peer.to_string());
            } else {
                let _ = out.send(err_msg("令牌无效，请重新配对"));
                return Action::CloseNow;
            }
        }
        // 免码配对：手机发 pair_request → 宿主弹原生确认框（物理在场模型）。
        // 拒绝/超时不关连接，App 可回退重试。
        "pair_request" => {
            let device = msg
                .get("device")
                .and_then(|v| v.as_str())
                .unwrap_or("移动设备")
                .to_string();
            let device_for_dialog = device.clone();
            log_info!("[remote] 收到 pair_request（{device}），弹确认框…");
            // 原生是/否弹窗（Win32 MessageBoxW，系统置顶）——阻塞调用放后台线程
            let allowed = tokio::task::spawn_blocking(move || {
                crate::win32::confirm_yes_no(
                    "电子声带 · 遥控配对",
                    &format!("允许「{device_for_dialog}」遥控这台电脑吗？"),
                )
            })
            .await
            .map_err(|e| format!("确认任务失败: {e}"));
            match allowed {
                Ok(Some(true)) => {
                    let token = {
                        let mut pairing = shared.pairing.lock().unwrap_or_else(|e| e.into_inner());
                        pairing.approve(&device)
                    };
                    let state = current_state(&shared.app).await;
                    let _ = out.send(
                        serde_json::json!({ "t": "pair_ok", "token": token, "state": state })
                            .to_string(),
                    );
                    *authenticated = true;
                    shared.take_over(out.clone(), Arc::clone(kill), peer.to_string());
                    log_info!("[remote] PC 端确认配对成功（{device}）");
                }
                Ok(Some(false)) => {
                    log_info!("[remote] PC 端拒绝配对（{device}）");
                    let _ = out.send(err_msg("PC 端拒绝了配对请求"));
                }
                Ok(None) => {
                    // 弹窗被关闭/无响应：不关连接，App 可重试
                    let _ = out.send(err_msg("配对确认框未响应，请重试"));
                }
                Err(e) => {
                    log_error!("[remote] 确认框异常: {e}");
                    let _ = out.send(err_msg(&e));
                }
            }
        }
        "ping" => {
            let _ = out.send(serde_json::json!({ "t": "pong" }).to_string());
        }

        // ── 以下需要鉴权 ──
        _ if !*authenticated => {
            let _ = out.send(err_msg("未配对：请先发送 pair_request 或 hello"));
            return Action::CloseNow;
        }
        "list_favorites" => {
            let out = out.clone();
            let app = shared.app.clone();
            tokio::spawn(async move {
                // native_list_favorites 恒返回 JSON 数组串（读盘，放后台线程）
                let json = tokio::task::spawn_blocking(move || native_list_favorites(&app))
                    .await
                    .unwrap_or_default();
                let items: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::json!([]));
                let _ = out.send(serde_json::json!({ "t": "favorites", "items": items }).to_string());
            });
        }
        "play_favorite" => {
            let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            spawn_blocking_ack(
                shared.app.clone(),
                out.clone(),
                ref_id,
                move |app| play_favorite_by_id(app, &id),
            );
        }
        "stop" => {
            spawn_blocking_ack(shared.app.clone(), out.clone(), ref_id, move |app| {
                native_stop_playback(app);
                Ok(())
            });
        }
        "synthesize" => {
            let text = msg
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let out2 = out.clone();
            let app = shared.app.clone();
            tokio::spawn(async move {
                // 合成是异步管线，直接 await（不再 block_on）
                let result = native_synthesize(&app, &text).await;
                let (ok, err) = match result {
                    Ok(()) => (true, String::new()),
                    Err(e) => (false, e),
                };
                if let Some(a) = ack(ref_id.as_deref(), ok, &err) {
                    let _ = out2.send(a);
                }
            });
        }
        "toggle_mic" => {
            let out2 = out.clone();
            let app = shared.app.clone();
            let app2 = shared.app.clone();
            tokio::spawn(async move {
                // 麦克风开关恒成功（后台线程翻转 + 读回状态）
                let _on = tokio::task::spawn_blocking(move || native_toggle_mic(&app))
                    .await
                    .unwrap_or(false);
                if let Some(a) = ack(ref_id.as_deref(), true, "") {
                    let _ = out2.send(a);
                }
                // 立即补一帧 state（麦克风开关是高频关注的态）
                if let Some(state) = current_state_opt(&app2).await {
                    let _ = out2.send(serde_json::json!({ "t": "state", "state": state }).to_string());
                }
            });
        }
        "play_last" => {
            spawn_blocking_ack(shared.app.clone(), out.clone(), ref_id, move |app| {
                native_play_last(app);
                Ok(())
            });
        }
        _ => {
            let _ = out.send(err_msg(&format!("未知消息类型「{t}」")));
        }
    }
    Action::Continue
}

/// 命令回执（请求带 ref 才回；ref 用于 App 侧关联请求与响应）
fn ack(ref_id: Option<&str>, ok: bool, err: &str) -> Option<String> {
    let r = ref_id?.to_string();
    Some(
        serde_json::json!({
            "t": "ack",
            "ref": r,
            "ok": ok,
            "err": if ok { "" } else { err },
        })
        .to_string(),
    )
}

/// 通用：spawn_blocking 跑一个宿主原生调用（同步、可能读盘/阻塞）→ 回 ack
fn spawn_blocking_ack<F>(
    app: AppHandle,
    out: mpsc::UnboundedSender<String>,
    ref_id: Option<String>,
    f: F,
) where
    F: FnOnce(&AppHandle) -> Result<(), String> + Send + 'static,
{
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || f(&app))
            .await
            .map_err(|e| format!("遥控任务崩溃: {e}"))
            .and_then(|r| r);
        let (ok, err) = match result {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e),
        };
        if let Some(a) = ack(ref_id.as_deref(), ok, &err) {
            let _ = out.send(a);
        }
    });
}

/// 查询当前状态（bridge::state_json），失败回退空对象
async fn current_state(app: &AppHandle) -> serde_json::Value {
    current_state_opt(app).await.unwrap_or(serde_json::json!({}))
}

async fn current_state_opt(app: &AppHandle) -> Option<serde_json::Value> {
    let app = app.clone();
    let json = tokio::task::spawn_blocking(move || state_json(&app)).await.ok()?;
    serde_json::from_str(&json).ok()
}
