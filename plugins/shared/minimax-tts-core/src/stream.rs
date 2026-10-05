// MiniMax 流式 TTS（WebSocket T2A v2）同步客户端。
//
// 官方协议（platform.minimax.io/docs/api-reference/speech-t2a-websocket，
// 国内站 platform.minimaxi.com/document/T2A V2 同协议）：
// - 连接 wss://api.minimaxi.com/ws/v1/t2a_v2（国内）/ wss://api.minimax.io/ws/v1/t2a_v2（国际），
//   请求头 Authorization: Bearer <api_key>；
// - 服务端先推 {"event":"connected_success"} → 客户端发 task_start（配置，无 text）
//   → 服务端 task_started → 客户端 task_continue（text，单条 ≤10000 字）→ task_finish
//   → 服务端逐块推 task_continued（data.audio 为 hex 编码音频，data 可能为 null，
//   is_final=true 表示本次请求音频结束）→ task_finished 后服务端关闭连接；
// - base_resp.status_code 非 0 即错误（1004 鉴权失败 / 2202 事件顺序 / 2205 队列积压等）；
//   task_failed 事件 = 终态失败；
// - 空闲 120 秒服务端自动断连（2201）——本会话一次文本数秒内完成，无需心跳。
//
// 音频块选 PCM（audio_setting.format="pcm"，16-bit 小端交错单声道 32kHz）：
// 分块字节流天然可在任意 2 字节对齐边界无缝续播，宿主无需 MP3 帧对齐。
//
// 线程模型：open() 派生 worker 线程独占 WebSocket；宿主经 next_chunk() 从
// channel 拉块（pull 模型）；abort() 只置停止位——next_chunk 轮询（100ms）
// 中立即感知返回，阻塞中的 worker 由「channel 断开/服务端关闭/停止位」收敛，
// 最迟 120 秒空闲断连时自清，不悬死宿主管线。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::header::AUTHORIZATION;
use tungstenite::http::HeaderValue;
use tungstenite::Message;

/// 流式输出采样率（PCM 16-bit 小端单声道）
pub const STREAM_SAMPLE_RATE: u32 = 32000;
/// 流式输出声道数
pub const STREAM_CHANNELS: u32 = 1;
/// task_continue 单条字符上限（官方硬顶 10000，留余量按句切分）
const TASK_CONTINUE_MAX_CHARS: usize = 4000;
/// 相邻音频块最大间隔：超过视为服务端失联（正常流式块间隔为亚秒级）
const CHUNK_GAP_TIMEOUT: Duration = Duration::from_secs(30);
/// next_chunk 轮询周期（决定 abort 生效时延的上限）
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// start 元信息 JSON（宿主据此装配播放器；当前契约只有 pcm_s16le）
pub fn stream_info_json() -> String {
    format!(
        r#"{{"format":"pcm_s16le","sample_rate":{},"channels":{}}}"#,
        STREAM_SAMPLE_RATE, STREAM_CHANNELS
    )
}

// ── 会话 ──────────────────────────────────────────────────

enum StreamItem {
    /// 一段 PCM 音频（已做 2 字节对齐）
    Chunk(Vec<u8>),
    /// 流正常结束
    Done,
    /// 终态失败（worker 退出前最后一条）
    Fail(String),
}

/// 一次流式合成会话。全内部可变，可放进 Arc 供注册表跨线程持有。
pub struct StreamSession {
    /// Receiver 非 Sync（不允许跨线程并发 recv），Mutex 包一层满足注册表的
    /// Sync 要求；宿主契约本就是单管线消费，锁只为类型成立，无实际争用。
    rx: Mutex<Receiver<StreamItem>>,
    stop: Arc<AtomicBool>,
    /// 最近一次收到服务端数据的时间（块间空闲超时基准）
    last_progress: Mutex<Instant>,
    /// worker 的终态错误（channel 断开时兜出，避免丢真实原因）
    terminal: Mutex<Option<String>>,
}

impl StreamSession {
    /// 打开会话：派生 worker 连接 MiniMax 并启动合成任务（本函数不拉音频，
    /// 连接级失败在此同步返回；任务级失败经 next_chunk 传出）。
    pub fn open(
        ws_base: &str,
        api_key: &str,
        model: &str,
        voice_id: &str,
        text: &str,
    ) -> Result<Self, String> {
        if text.trim().is_empty() {
            return Err("文本为空，无法合成".into());
        }
        let (tx, rx) = channel::<StreamItem>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        // 全部转为 String 再进闭包：闭包按值捕获，不借用函数参数生命周期
        let ws_base = ws_base.to_string();
        let api_key = api_key.to_string();
        let model = model.to_string();
        let voice_id = voice_id.to_string();
        let worker_text = text.to_string();
        let handle = std::thread::Builder::new()
            .name("minimax-tts-ws".into())
            .spawn(move || worker_run(worker_stop, tx, ws_base, api_key, model, voice_id, worker_text))
            .map_err(|e| format!("启动流式合成线程失败: {e}"))?;
        // 句柄仅用于命名观察，不 join（Drop 语义：分离，worker 自收敛）
        std::mem::forget(handle);
        Ok(Self {
            rx: Mutex::new(rx),
            stop,
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        })
    }

    /// 拉取下一个音频块（阻塞）。
    /// Ok(非空) = 一块 PCM；Ok(空) = 流正常结束；Err = 失败或已中止（中文原因）。
    pub fn next_chunk(&self) -> Result<Vec<u8>, String> {
        loop {
            let recv = self
                .rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(POLL_INTERVAL);
            match recv {
                Ok(StreamItem::Chunk(bytes)) => {
                    *self.last_progress.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    return Ok(bytes);
                }
                Ok(StreamItem::Done) => return Ok(Vec::new()),
                Ok(StreamItem::Fail(msg)) => {
                    *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                    return Err(msg);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.stop.load(Ordering::Relaxed) {
                        return Err("流式合成已中止".into());
                    }
                    let idle = self.last_progress.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    if idle > CHUNK_GAP_TIMEOUT {
                        let msg = format!(
                            "流式服务 {} 秒未返回音频，连接中断",
                            CHUNK_GAP_TIMEOUT.as_secs()
                        );
                        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                        return Err(msg);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // worker 已退出：优先兜出真实终态错误
                    let stored = self
                        .terminal
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    return Err(stored.unwrap_or_else(|| "流式会话已关闭".into()));
                }
            }
        }
    }

    /// 中止会话（幂等；可从任意线程调用）。next_chunk 轮询中 ≤100ms 感知返回。
    /// 阻塞中的 worker 不会立即退出：等 channel 断开、服务端返回或 120 秒
    /// 空闲断连时自清（见模块头注释），不持有宿主资源。
    pub fn abort(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ── worker：独占 WebSocket 的任务执行 ─────────────────────

fn worker_run(
    stop: Arc<AtomicBool>,
    tx: Sender<StreamItem>,
    ws_base: String,
    api_key: String,
    model: String,
    voice_id: String,
    text: String,
) {
    // 宿主放弃（接收端已 drop）即无产出意义，各阶段发送失败直接收敛退出
    let emit = |tx: &Sender<StreamItem>, item: StreamItem| -> bool { tx.send(item).is_ok() };

    // 1. 连接（含 TLS 与握手；阻塞调用，DNS/TCP 由系统超时兜底）
    let url = format!("{}/ws/v1/t2a_v2", ws_base.trim_end_matches('/'));
    let mut request = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            let _ = emit(&tx, StreamItem::Fail(format!("流式地址非法 {url}: {e}")));
            return;
        }
    };
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
        request.headers_mut().insert(AUTHORIZATION, v);
    }
    let (mut ws, _resp) = match tungstenite::connect(request) {
        Ok(x) => x,
        Err(e) => {
            let _ = emit(&tx, StreamItem::Fail(format!("连接 MiniMax 流式服务失败: {e}")));
            return;
        }
    };

    // 2. 握手序：connected_success → 发 task_start → task_started
    if let Err(e) = expect_event(&mut ws, &stop, "connected_success") {
        let _ = emit(&tx, StreamItem::Fail(e));
        return;
    }
    let task_start = serde_json::json!({
        "event": "task_start",
        "model": model,
        "voice_setting": { "voice_id": voice_id, "speed": 1.0, "vol": 1.0, "pitch": 0 },
        "audio_setting": {
            "sample_rate": STREAM_SAMPLE_RATE,
            "format": "pcm",
            "channel": STREAM_CHANNELS,
        },
    });
    if let Err(e) = ws.send(Message::text(task_start.to_string())) {
        let _ = emit(&tx, StreamItem::Fail(format!("发送 task_start 失败: {e}")));
        return;
    }
    if let Err(e) = expect_event(&mut ws, &stop, "task_started") {
        let _ = emit(&tx, StreamItem::Fail(e));
        return;
    }

    // 3. 分句下发文本 + 收尾（官方：task_start 后可多次 task_continue，≤10000 字/条）
    for piece in split_text(&text, TASK_CONTINUE_MAX_CHARS) {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let msg = serde_json::json!({ "event": "task_continue", "text": piece });
        if let Err(e) = ws.send(Message::text(msg.to_string())) {
            let _ = emit(&tx, StreamItem::Fail(format!("发送 task_continue 失败: {e}")));
            return;
        }
    }
    if let Err(e) = ws.send(Message::text(r#"{"event":"task_finish"}"#)) {
        let _ = emit(&tx, StreamItem::Fail(format!("发送 task_finish 失败: {e}")));
        return;
    }

    // 4. 收流：task_continued（hex 音频块）直到 is_final / task_finished / 失败
    let mut pending_byte: Vec<u8> = Vec::new(); // PCM 2 字节对齐余量
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let msg = match ws.read() {
            Ok(m) => m,
            Err(e) => {
                // 服务端发完 task_finished 后主动断开属正常收尾，不算失败
                if matches!(e, tungstenite::Error::ConnectionClosed) {
                    let _ = emit(&tx, StreamItem::Done);
                } else {
                    let _ = emit(&tx, StreamItem::Fail(format!("流式连接中断: {e}")));
                }
                return;
            }
        };
        match msg {
            Message::Text(raw) => {
                match serde_json::from_str::<WsEvent>(&raw) {
                    Ok(ev) => {
                        // 统一业务错误出口：任何事件携带非 0 base_resp 均为终态失败
                        if let Some(br) = &ev.base_resp {
                            if br.status_code != 0 {
                                let _ = emit(&tx, StreamItem::Fail(format!(
                                    "MiniMax 流式错误 {}: {}",
                                    br.status_code,
                                    br.status_msg.as_deref().unwrap_or("未知错误")
                                )));
                                return;
                            }
                        }
                        match ev.event.as_deref() {
                            Some("task_continued") => {
                                let hex = ev.data.as_ref().and_then(|d| d.audio.as_deref());
                                if let Some(hex) = hex {
                                    if !hex.is_empty() {
                                        match hex_decode(hex) {
                                            Ok(mut bytes) => {
                                                pending_byte.append(&mut bytes);
                                                // 补齐偶数字节（16-bit PCM），尾字节留待下块
                                                let aligned =
                                                    pending_byte.len() - pending_byte.len() % 2;
                                                let out: Vec<u8> =
                                                    pending_byte.drain(..aligned).collect();
                                                if !out.is_empty()
                                                    && !emit(&tx, StreamItem::Chunk(out))
                                                {
                                                    return; // 宿主已放弃
                                                }
                                            }
                                            Err(e) => {
                                                let _ = emit(&tx, StreamItem::Fail(e));
                                                return;
                                            }
                                        }
                                    }
                                }
                                if ev.is_final {
                                    // 音频结束（此后服务端还会补发 task_finished，下一轮读收尾）
                                    let _ = emit(&tx, StreamItem::Done);
                                    return;
                                }
                            }
                            Some("task_finished") => {
                                let _ = emit(&tx, StreamItem::Done);
                                return;
                            }
                            Some("task_failed") => {
                                let reason = ev
                                    .base_resp
                                    .and_then(|b| b.status_msg)
                                    .unwrap_or_else(|| "未知错误".into());
                                let _ =
                                    emit(&tx, StreamItem::Fail(format!("合成任务失败: {reason}")));
                                return;
                            }
                            _ => {} // connected_success / task_started 重复帧等，忽略
                        }
                    }
                    Err(e) => {
                        let _ = emit(&tx, StreamItem::Fail(format!("流式消息解析失败: {e}")));
                        return;
                    }
                }
            }
            Message::Ping(p) => {
                // 官方服务端不主动 ping；对端 ping 必须回 pong 维持连接
                let _ = ws.send(Message::Pong(p));
            }
            Message::Close(_) => {
                let _ = emit(&tx, StreamItem::Done); // 服务端正常收尾路径
                return;
            }
            Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }
}

/// 读消息直到收到指定事件（跳过 ping/pong/无关帧）；非 0 base_resp 即失败。
fn expect_event(
    ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    stop: &AtomicBool,
    event: &str,
) -> Result<(), String> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("流式合成已中止".into());
        }
        let msg = ws
            .read()
            .map_err(|e| format!("等待 {event} 时连接失败: {e}"))?;
        match msg {
            Message::Text(raw) => {
                let ev: WsEvent = serde_json::from_str(&raw)
                    .map_err(|e| format!("流式消息解析失败: {e}"))?;
                if let Some(br) = &ev.base_resp {
                    if br.status_code != 0 {
                        return Err(format!(
                            "MiniMax 流式错误 {}: {}",
                            br.status_code,
                            br.status_msg.as_deref().unwrap_or("未知错误")
                        ));
                    }
                }
                if ev.event.as_deref() == Some(event) {
                    return Ok(());
                }
            }
            Message::Ping(p) => {
                let _ = ws.send(Message::Pong(p));
            }
            Message::Close(_) => {
                return Err(format!("服务端在握手阶段关闭连接（等待 {event}）"));
            }
            Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }
}

// ── 协议结构 ──────────────────────────────────────────────

#[derive(Deserialize)]
struct WsEvent {
    event: Option<String>,
    #[serde(default)]
    is_final: bool,
    data: Option<WsData>,
    base_resp: Option<BaseRespMsg>,
}

#[derive(Deserialize)]
struct WsData {
    audio: Option<String>,
}

#[derive(Deserialize)]
struct BaseRespMsg {
    status_code: i32,
    status_msg: Option<String>,
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex 字符串长度为奇数".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("hex 解码错误 @{}: {}", i, e))
        })
        .collect()
}

// ── 文本分句（task_continue 单条长度限制）────────────────

/// 把文本切成每段不超过 max_chars 的片段：优先在句读符号后断开，
/// 无符号超长时硬切。空文本返回空表。
fn split_text(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return if text.is_empty() { Vec::new() } else { vec![text.to_string()] };
    }
    let breakpoints: &[char] = &['。', '！', '？', '；', '.', '!', '?', ';', '\n'];
    let mut pieces = Vec::new();
    let mut start = 0usize;
    while chars.len() - start > max_chars {
        // 在 (start, start+max_chars] 窗口内找最后一个句读位（含该字符）
        let mut cut = None;
        for i in (start + 1..=start + max_chars).rev() {
            if breakpoints.contains(&chars[i - 1]) {
                cut = Some(i);
                break;
            }
        }
        let end = cut.unwrap_or(start + max_chars);
        pieces.push(chars[start..end].iter().collect());
        start = end;
    }
    if start < chars.len() {
        pieces.push(chars[start..].iter().collect());
    }
    pieces
}

// ── 插件侧会话注册表（u64 句柄 → 会话）──────────────────

/// 全局会话表：engine crate 经此实现 va_tts_stream_plugin! 的 u64 句柄语义。
pub struct SessionRegistry {
    map: Mutex<HashMap<u64, Arc<StreamSession>>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 注册新会话并返回句柄
    pub fn insert(&self, session: StreamSession) -> u64 {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::new(session));
        id
    }

    /// 拉取一个块；会话到达终态（结束/失败/中止）时自动移除
    pub fn next_chunk(&self, id: u64) -> Result<Vec<u8>, String> {
        let session = {
            let map = self.map.lock().unwrap_or_else(|e| e.into_inner());
            map.get(&id).cloned()
        };
        let Some(session) = session else {
            return Err("流式会话不存在或已停止".into());
        };
        let result = session.next_chunk();
        if matches!(&result, Ok(v) if v.is_empty()) || result.is_err() {
            self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
        result
    }

    /// 中止并释放会话（幂等）
    pub fn abort(&self, id: u64) {
        if let Some(session) = self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            session.abort();
        }
    }
}

/// 进程级单例会话表（engine crate 用）
pub fn global_sessions() -> &'static SessionRegistry {
    static REGISTRY: OnceLock<SessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SessionRegistry::new)
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_短文本不切() {
        assert_eq!(split_text("你好世界", 10), vec!["你好世界"]);
        assert_eq!(split_text("", 10), Vec::<String>::new());
    }

    #[test]
    fn split_优先句读断开() {
        let text = "第一句。第二句！第三句？第四句。";
        // 5 字上限：在句读后断开
        let pieces = split_text(text, 5);
        assert_eq!(pieces.join(""), text);
        assert!(pieces.iter().all(|p| p.chars().count() <= 5));
        assert_eq!(pieces, vec!["第一句。", "第二句！", "第三句？", "第四句。"]);
    }

    #[test]
    fn split_无符号硬切() {
        let text = "abcdefghij"; // 10 个字母，无句读
        let pieces = split_text(text, 4);
        assert_eq!(pieces, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn split_按字符数不按字节() {
        let text = "中文段落。中文段落。"; // 全中文
        let pieces = split_text(text, 6);
        assert_eq!(pieces.join(""), text);
        assert!(pieces.iter().all(|p| p.chars().count() <= 6));
    }

    #[test]
    fn ws_event_解析_task_continued终帧() {
        let raw = r#"{
            "data": { "audio": "deadbeef" },
            "extra_info": { "audio_length": 9914, "usage_characters": 158 },
            "is_final": true,
            "session_id": "301871346491491",
            "base_resp": { "status_code": 0, "status_msg": "success" }
        }"#;
        let ev: WsEvent = serde_json::from_str(raw).unwrap();
        assert!(ev.is_final);
        assert_eq!(ev.data.unwrap().audio.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn ws_event_解析_data为null不报错() {
        // 官方文档明确 data 可能为 null
        let raw = r#"{"event":"task_continued","data":null,"is_final":false,
                      "base_resp":{"status_code":0}}"#;
        let ev: WsEvent = serde_json::from_str(raw).unwrap();
        assert!(ev.data.is_none());
        assert!(!ev.is_final);
    }

    #[test]
    fn ws_event_解析错误码() {
        let raw = r#"{"event":"task_failed","base_resp":{"status_code":1004,"status_msg":"invalid api key"}}"#;
        let ev: WsEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(ev.base_resp.unwrap().status_code, 1004);
    }

    #[test]
    fn hex_decode_流式块() {
        assert_eq!(hex_decode("00ff10").unwrap(), vec![0, 255, 16]);
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn stream_info_json_格式契约() {
        let info: serde_json::Value = serde_json::from_str(&stream_info_json()).unwrap();
        assert_eq!(info["format"], "pcm_s16le");
        assert_eq!(info["sample_rate"], 32000);
        assert_eq!(info["channels"], 1);
    }
}
