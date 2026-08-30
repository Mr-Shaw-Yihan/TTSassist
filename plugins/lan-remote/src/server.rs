// WebSocket 服务器与遥控协议（协议契约见 doc/移动端遥控器设计.md §三）。
//
// 模型：
// - 一对一：同一时刻只有一个已鉴权连接，新配对/新 hello 顶替旧会话；
// - 未鉴权连接只允许 pair_request / hello / ping，其余回 error 并断开
//   （配对码路径 pair / refresh_code 已随 1.8.x 移除，收到即回错误并断开）；
// - 桥能力调用全部走 spawn_blocking（C ABI 阻塞调用，不堵 WS 事件循环）；
// - 宿主事件（EVENT_RX）转发给已鉴权连接：先透传 event，再重查 state 推送。

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message;

use crate::pairing::Pairing;

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
    let (status, ctype, body) = crate::web::http_response(path);
    eprintln!("[lan-remote] Web 遥控页面请求: {path} → {status}");
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
}

/// 全局共享状态
pub struct Shared {
    authed: Mutex<Option<AuthedConn>>,
    pub pairing: Mutex<Pairing>,
}

impl Shared {
    pub fn new(pairing: Pairing) -> Self {
        Self {
            authed: Mutex::new(None),
            pairing: Mutex::new(pairing),
        }
    }

    /// 注册新的已鉴权连接，顶替并关闭旧连接（一对一）
    fn take_over(&self, out: mpsc::UnboundedSender<String>, kill: Arc<Notify>) {
        let old = {
            let mut guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
            guard.replace(AuthedConn { out, kill })
        };
        if let Some(old) = old {
            old.kill.notify_waiters();
            eprintln!("[lan-remote] 新会话已连接，旧会话被顶替");
        }
    }

    /// 给已鉴权连接发一条 JSON（无连接时丢弃）
    fn send_authed(&self, json: String) {
        let guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conn) = guard.as_ref() {
            let _ = conn.out.send(json);
        }
    }

    /// 连接断开后清理（只清理仍指向自己的句柄）
    fn clear_authed(&self, out: &mpsc::UnboundedSender<String>) {
        let mut guard = self.authed.lock().unwrap_or_else(|e| e.into_inner());
        let is_current = guard.as_ref().map(|c| c.out.same_channel(out)) == Some(true);
        if is_current {
            *guard = None;
        }
    }
}

/// 服务器主循环：接受连接 + 转发宿主事件。占用当前任务直至进程退出。
pub async fn run(listener: TcpListener, shared: Arc<Shared>, mut event_rx: mpsc::UnboundedReceiver<String>) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, addr)) => {
                        let shared = Arc::clone(&shared);
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, shared).await {
                                eprintln!("[lan-remote] 连接 {addr} 结束: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[lan-remote] 接受连接失败: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            // 宿主事件 → 已鉴权连接：透传 event + 重查 state 推送
            Some(event) = event_rx.recv() => {
                let shared = Arc::clone(&shared);
                // 透传事件原样
                let evt_msg = serde_json::json!({ "t": "event", "event": serde_json::from_str::<serde_json::Value>(&event).unwrap_or(serde_json::Value::Null) });
                shared.send_authed(evt_msg.to_string());
                // 重查状态并推送（桥调用阻塞，放后台线程）
                let state = tokio::task::spawn_blocking(|| plugin_api::host_bridge::get_state_json().unwrap_or_default()).await.unwrap_or_default();
                if !state.is_empty() {
                    shared.send_authed(serde_json::json!({ "t": "state", "state": serde_json::from_str::<serde_json::Value>(&state).unwrap_or(serde_json::Value::Null) }).to_string());
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
    eprintln!("[lan-remote] WS 握手成功（{peer}）");
    let (mut sink, mut source) = ws.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let kill = Arc::new(Notify::new());
    let mut authenticated = false;
    let mut closed = false;

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
            // 新会话顶替：立即关闭本连接
            _ = kill.notified() => {
                let _ = sink.send(Message::Close(None)).await;
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
                let action = handle_message(&text, &shared, &out_tx, &kill, &mut authenticated).await;
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

    if authenticated {
        shared.clear_authed(&out_tx);
        eprintln!("[lan-remote] 已鉴权连接断开（{peer}）");
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
            let ok = {
                let pairing = shared.pairing.lock().unwrap_or_else(|e| e.into_inner());
                pairing.check_token(token)
            };
            if ok {
                let state = current_state().await;
                let resp = serde_json::json!({
                    "t": "hello_ok",
                    "state": state,
                });
                let _ = out.send(resp.to_string());
                *authenticated = true;
                shared.take_over(out.clone(), Arc::clone(kill));
            } else {
                let _ = out.send(err_msg("令牌无效，请重新配对"));
                return Action::CloseNow;
            }
        }
        // 免码配对：手机发 pair_request → 宿主弹原生确认框（物理在场模型）。
        // 拒绝/超时不关连接，App 可回退到配对码流程。
        "pair_request" => {
            let device = msg
                .get("device")
                .and_then(|v| v.as_str())
                .unwrap_or("移动设备")
                .to_string();
            let device_for_dialog = device.clone();
            eprintln!("[lan-remote] 收到 pair_request（{device}），弹确认框…");
            let allowed = tokio::task::spawn_blocking(move || {
                plugin_api::host_bridge::confirm_dialog(
                    "电子声带 · 遥控配对",
                    &format!("允许「{device_for_dialog}」遥控这台电脑吗？"),
                )
            })
            .await
            .map_err(|e| format!("确认任务失败: {e}"))
            .and_then(|r| r);
            match allowed {
                Ok(true) => {
                    let token = {
                        let mut pairing = shared.pairing.lock().unwrap_or_else(|e| e.into_inner());
                        pairing.approve()
                    };
                    let state = current_state().await;
                    let _ = out.send(
                        serde_json::json!({ "t": "pair_ok", "token": token, "state": state })
                            .to_string(),
                    );
                    *authenticated = true;
                    shared.take_over(out.clone(), Arc::clone(kill));
                    eprintln!("[lan-remote] PC 端确认配对成功（{device}）");
                }
                Ok(false) => {
                    eprintln!("[lan-remote] PC 端拒绝配对（{device}）");
                    let _ = out.send(err_msg("PC 端拒绝了配对请求"));
                }
                Err(e) => {
                    eprintln!("[lan-remote] 确认框异常: {e}");
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
            tokio::spawn(async move {
                let json = tokio::task::spawn_blocking(|| {
                    plugin_api::host_bridge::list_favorites_json().unwrap_or_else(|e| e.to_string())
                })
                .await
                .unwrap_or_default();
                // 失败时 json 是错误文本（非数组），包进 error 回执
                if json.trim_start().starts_with('[') {
                    let items: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::json!([]));
                    let _ = out.send(serde_json::json!({ "t": "favorites", "items": items }).to_string());
                } else {
                    if let Some(r) = ack(ref_id.as_deref(), false, &json) {
                        let _ = out.send(r);
                    }
                }
            });
        }
        "play_favorite" => {
            let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            spawn_bridge(out.clone(), ref_id, move || {
                plugin_api::host_bridge::play_favorite(&id)
            });
        }
        "stop" => {
            spawn_bridge(out.clone(), ref_id, move || {
                plugin_api::host_bridge::stop_playback()
            });
        }
        "synthesize" => {
            let text = msg
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            spawn_bridge(out.clone(), ref_id, move || {
                plugin_api::host_bridge::synthesize(&text)
            });
        }
        "toggle_mic" => {
            let out2 = out.clone();
            let ref2 = ref_id.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(|| plugin_api::host_bridge::toggle_mic_send())
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                // ack + 立即补一帧 state（麦克风开关是高频关注的态）
                let (ok, err) = match &result {
                    Ok(_) => (true, String::new()),
                    Err(e) => (false, e.clone()),
                };
                if let Some(a) = ack(ref2.as_deref(), ok, &err) {
                    let _ = out2.send(a);
                }
                if let Some(state) = current_state_opt().await {
                    let _ = out2.send(serde_json::json!({ "t": "state", "state": state }).to_string());
                }
            });
        }
        "play_last" => {
            spawn_bridge(out.clone(), ref_id, move || {
                plugin_api::host_bridge::play_last()
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

/// 通用：spawn_blocking 跑一个桥调用 → 回 ack
fn spawn_bridge<F>(out: mpsc::UnboundedSender<String>, ref_id: Option<String>, f: F)
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| format!("插件任务崩溃: {e}"))
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

/// 查询当前状态（桥 get_state），失败回退空对象
async fn current_state() -> serde_json::Value {
    current_state_opt().await.unwrap_or(serde_json::json!({}))
}

async fn current_state_opt() -> Option<serde_json::Value> {
    let json = tokio::task::spawn_blocking(|| plugin_api::host_bridge::get_state_json().ok())
        .await
        .ok()
        .flatten()?;
    serde_json::from_str(&json).ok()
}
