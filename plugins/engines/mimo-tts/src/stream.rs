// MiMo 流式会话：HTTP SSE → 宿主拉模型（va_tts_stream_next）。
//
// ABI 是宿主主动拉块，而 MiMo 是「一次 POST、服务端连续推 SSE」，故：
// worker 线程独占响应体读取 → mpsc 队列 → next_chunk 轮询取块（100ms 周期）。
// abort 只置停止位：next_chunk 轮询里 ≤100ms 返回错误；worker 阻塞在 read_until
// 上不会立刻醒，但宿主此时已不再取块、连接随请求超时兜底自清，不悬死。
//
// 骨架与 minimax-tts-core/src/stream.rs 同构（会话字段/轮询常量/注册表照搬），
// 只把 WS 换成 blocking HTTP + BufReader::read_until(b'\n')。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::client::{align_bytes, sse_line_to_pcm, strip_wav_container, ENDPOINT};

/// 相邻音频块最大间隔：超过视为服务端失联（正常流式块间隔为亚秒级）
const CHUNK_GAP_TIMEOUT: Duration = Duration::from_secs(30);
/// next_chunk 轮询周期（决定 abort 生效时延上限）
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 整请求上限：MiMo 一次合成长文本远低于此值，仅作为 worker 卡死时的兜底
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

enum StreamItem {
    /// 一段 PCM（已在 worker 里做 2 字节对齐）
    Chunk(Vec<u8>),
    /// 流正常结束
    Done,
    /// 终态失败（worker 退出前最后一条）
    Fail(String),
}

/// 一次流式合成会话。全内部可变，可放进 Arc 供注册表跨线程持有。
pub struct StreamSession {
    /// Receiver 非 Sync，Mutex 包一层只为让注册表的 Sync 成立（宿主契约是单管线消费）
    rx: Mutex<Receiver<StreamItem>>,
    stop: Arc<AtomicBool>,
    /// 最近一次拿到数据块的时间（块间空闲超时基准）
    last_progress: Mutex<Instant>,
    /// worker 终态错误（channel 断开时兜出，避免丢真实原因）
    terminal: Mutex<Option<String>>,
}

impl StreamSession {
    /// 测试注入用：不起 worker，直接给一个接收端
    #[cfg(test)]
    fn from_receiver(rx: Receiver<StreamItem>) -> Self {
        Self {
            rx: Mutex::new(rx),
            stop: Arc::new(AtomicBool::new(false)),
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        }
    }

    /// 打开会话：**同步**发出 POST 并校验响应状态（Key 错/连不上在这里返回中文错），
    /// 成功再把响应体交给 worker 逐行读。
    pub fn open(api_key: String, body: serde_json::Value) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;
        let resp = client
            .post(ENDPOINT)
            .header("api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("连接 MiMo 失败：{e}（请检查网络与 API Key）"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(format!("MiMo 返回 {status}：{}", trim_for_msg(&text)));
        }

        let (tx, rx) = channel::<StreamItem>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("mimo-tts-sse".into())
            .spawn(move || worker_run(worker_stop, tx, resp))
            .map_err(|e| format!("启动流式读取线程失败: {e}"))?;
        // 句柄只用于给线程命名，不 join（worker 自收敛）
        std::mem::forget(handle);

        Ok(Self {
            rx: Mutex::new(rx),
            stop,
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        })
    }

    /// 拉取下一块。Ok(非空)=一块 PCM；Ok(空)=流正常结束；Err=失败或已中止（中文原因）。
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
                        return Err("MiMo 流式合成已中止".into());
                    }
                    let idle = self.last_progress.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    if idle > CHUNK_GAP_TIMEOUT {
                        let msg = format!(
                            "MiMo 流式服务 {} 秒未返回音频，连接中断",
                            CHUNK_GAP_TIMEOUT.as_secs()
                        );
                        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                        return Err(msg);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
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

    /// 中止（幂等，任意线程可调）。next_chunk 轮询中 ≤100ms 感知返回。
    pub fn abort(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// worker：独占 SSE 响应体，按行解析、剥 WAV、2 字节对齐后推块
fn worker_run(stop: Arc<AtomicBool>, tx: Sender<StreamItem>, resp: reqwest::blocking::Response) {
    // reqwest::blocking::Response 本身实现了 std::io::Read（无 take() 方法），直接包 BufReader
    let mut reader = BufReader::new(resp);
    let mut line: Vec<u8> = Vec::new();
    let mut carry: Option<u8> = None;
    let mut got_audio = false;
    // 首块才做 WAV 探测：真流式的后续块是裸 PCM，不该被误当容器解析
    let mut first = true;

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = tx.send(StreamItem::Fail("MiMo 流式合成已中止".into()));
            return;
        }
        line.clear();
        let n = match reader.read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(e) => {
                let _ = tx.send(StreamItem::Fail(format!("MiMo 流式读取失败: {e}")));
                return;
            }
        };
        if n == 0 {
            break; // EOF：服务端收流
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if !feed_line_or_fail(&tx, text, &mut carry, &mut first, &mut got_audio) {
            return; // 已发出 Fail
        }
    }

    // 收尾：服务端最后一个 chunk 可能不带换行
    if !line.is_empty() {
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if !feed_line_or_fail(&tx, text, &mut carry, &mut first, &mut got_audio) {
            return;
        }
    }

    if !got_audio {
        let _ = tx.send(StreamItem::Fail(
            "MiMo 流式未返回音频（文本可能为空或全是符号）".into(),
        ));
        return;
    }
    let _ = tx.send(StreamItem::Done);
}

/// 处理一行 SSE。返回 false 表示已发送 Fail、worker 应退出。
fn feed_line_or_fail(
    tx: &Sender<StreamItem>,
    line: &str,
    carry: &mut Option<u8>,
    first: &mut bool,
    got_audio: &mut bool,
) -> bool {
    let bytes = match sse_line_to_pcm(line) {
        None => return true,
        Some(Err(e)) => {
            let _ = tx.send(StreamItem::Fail(e));
            return false;
        }
        Some(Ok(b)) => b,
    };
    let mut bytes = bytes;
    if *first {
        // 兼容模式可能整段返回 WAV：剥掉容器，保证交给宿主的仍是裸 PCM
        if let Some(pcm) = strip_wav_container(&bytes) {
            bytes = pcm;
        }
        *first = false;
    }
    let aligned = align_bytes(carry, &bytes);
    if aligned.is_empty() {
        return true;
    }
    *got_audio = true;
    tx.send(StreamItem::Chunk(aligned)).is_ok()
}

/// 错误消息进用户弹窗前必须剪短（HTTP body 可能是一大坨 HTML/JSON）
fn trim_for_msg(s: &str) -> String {
    const CAP: usize = 200;
    let flat: String = s.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
    if flat.chars().count() <= CAP {
        return flat;
    }
    let cut: String = flat.chars().take(CAP).collect();
    format!("{cut}…")
}

// ── 注册表 ────────────────────────────────────────────────

/// 会话注册表：句柄是 u64（跨 FFI 传 `VaStreamSession` 时用 `id as *mut c_void`）
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
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::new(session));
        id
    }

    /// 拉一个块；会话到终态（结束/失败/中止）时自动移除
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

/// 进程级单例会话表
pub fn global_sessions() -> &'static SessionRegistry {
    static REGISTRY: OnceLock<SessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SessionRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 注册表_句柄自增且终态自动摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx);
        let id = reg.insert(session);
        assert_eq!(id, 1);
        let id2 = reg.insert(StreamSession::from_receiver(channel::<StreamItem>().1));
        assert_eq!(id2, 2);
        // Done → 拿到空 vec 且会话被摘除
        tx.send(StreamItem::Done).unwrap();
        let got = reg.next_chunk(id).unwrap();
        assert!(got.is_empty(), "Done 应返回空 vec 表示流正常结束");
        assert_eq!(reg.next_chunk(id).unwrap_err(), "流式会话不存在或已停止");
    }

    #[test]
    fn 注册表_失败透出中文原因并摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let id = reg.insert(StreamSession::from_receiver(rx));
        tx.send(StreamItem::Fail("MiMo 流式失败".into())).unwrap();
        assert_eq!(reg.next_chunk(id).unwrap_err(), "MiMo 流式失败");
        assert!(reg.next_chunk(id).is_err());
    }

    #[test]
    fn abort_置位后轮询亚秒返回错误() {
        let (_tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx);
        session.abort();
        let t = std::time::Instant::now();
        let err = session.next_chunk().unwrap_err();
        assert!(err.contains("中止"), "实际: {err}");
        assert!(t.elapsed() < Duration::from_millis(500), "abort 应亚秒生效，实际 {:?}", t.elapsed());
    }

    #[test]
    fn info契约_pcm_s16le_24k_单声道() {
        let info: serde_json::Value = serde_json::from_str(&crate::client::stream_info_json()).unwrap();
        assert_eq!(info["format"], "pcm_s16le");
        assert_eq!(info["sample_rate"], 24000);
        assert_eq!(info["channels"], 1);
    }
}
