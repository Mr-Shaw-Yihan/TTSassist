// MOSS 流式会话：HTTP 分块裸 PCM → 宿主拉模型（va_tts_stream_next）。
//
// ABI 是宿主主动拉块，而 MOSS 是「一次 POST、服务端连续推裸 PCM 字节」，故：
// worker 线程独占响应体读取 → 2 字节对齐 → mpsc 队列 → next_chunk 轮询取块（100ms 周期）。
// abort 只置停止位：next_chunk 轮询里 ≤100ms 返回错误；worker 阻塞在 read 上不会立刻醒，
// 但宿主此时已不再取块、连接随请求超时兜底自清，不悬死。
//
// 骨架与 mimo-tts/src/stream.rs 同构（会话字段/轮询常量/注册表照搬），
// 只把「SSE 逐行解析」换成「裸字节读取 + 奇偶对齐」，并多带一份从响应头解析出的音频参数。

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::client::{align_bytes, AudioParams, ENDPOINT};

/// 相邻音频块最大间隔：超过视为服务端失联（正常流式块间隔为亚秒级）
const CHUNK_GAP_TIMEOUT: Duration = Duration::from_secs(30);
/// 首块前的等待上限。实测非流式整段合成（七八个字）要 5.3s，流式首块应远快于此；
/// 给到 60s 是为长文本留余量，同时仍小于 REQUEST_TIMEOUT，死连接不会永挂。
const FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
/// next_chunk 轮询周期（决定 abort 生效时延上限）
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 整请求上限：仅作为 worker 卡死时的兜底
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// 响应体读取缓冲（16KB，够宿主即时起播）
const READ_BUF: usize = 16 * 1024;

enum StreamItem {
    /// 一段已 2 字节对齐的 PCM
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
    /// 是否已交出过首块（决定用首块窗口还是块间窗口）
    got_first: AtomicBool,
    /// worker 终态错误（channel 断开时兜出，避免丢真实原因）
    terminal: Mutex<Option<String>>,
    /// 打开会话时从**响应头**解析到的音频参数（文档：客户端必须据响应头解释音频参数）
    pub params: AudioParams,
}

impl StreamSession {
    /// 测试注入用：不起 worker，直接给一个接收端
    #[cfg(test)]
    fn from_receiver(rx: Receiver<StreamItem>, params: AudioParams) -> Self {
        Self {
            rx: Mutex::new(rx),
            stop: Arc::new(AtomicBool::new(false)),
            last_progress: Mutex::new(Instant::now()),
            got_first: AtomicBool::new(false),
            terminal: Mutex::new(None),
            params,
        }
    }

    /// 打开会话：**同步**发出 POST 并校验响应状态（Key 错/模型不支持流式在这里就返回中文错），
    /// 成功后先读响应头定音频参数，再把响应体交给 worker 逐块读。
    pub fn open(api_key: String, body: serde_json::Value) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;
        let resp = client
            .post(ENDPOINT)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("连接 MOSS 失败：{e}（请检查网络与 API Key）"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(crate::client::friendly_error(status.as_u16(), &text));
        }
        let params = crate::client::parse_audio_params(
            resp.headers()
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str(), s))),
        );

        let (tx, rx) = channel::<StreamItem>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("moss-tts-pcm".into())
            .spawn(move || worker_run(worker_stop, tx, resp))
            .map_err(|e| format!("启动流式读取线程失败: {e}"))?;
        // 句柄只用于给线程命名，不 join（worker 自收敛）
        std::mem::forget(handle);

        Ok(Self {
            rx: Mutex::new(rx),
            stop,
            last_progress: Mutex::new(Instant::now()),
            got_first: AtomicBool::new(false),
            terminal: Mutex::new(None),
            params,
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
                    self.got_first.store(true, Ordering::Relaxed);
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
                        return Err("MOSS 流式合成已中止".into());
                    }
                    let idle = self
                        .last_progress
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .elapsed();
                    let limit = if self.got_first.load(Ordering::Relaxed) {
                        CHUNK_GAP_TIMEOUT
                    } else {
                        FIRST_CHUNK_TIMEOUT
                    };
                    if idle > limit {
                        let msg = format!("MOSS 流式服务 {} 秒未返回音频，连接中断", limit.as_secs());
                        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                        return Err(msg);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let stored = self.terminal.lock().unwrap_or_else(|e| e.into_inner()).clone();
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

/// worker：独占响应体，读裸 PCM、2 字节对齐后推块
fn worker_run(stop: Arc<AtomicBool>, tx: Sender<StreamItem>, reader: impl Read) {
    let mut buf = vec![0u8; READ_BUF];
    let mut carry: Option<u8> = None;
    let mut got_audio = false;
    // reqwest::blocking::Response 本身实现了 Read，直接读即可（无需再包 BufReader）
    let mut reader = reader;

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = tx.send(StreamItem::Fail("MOSS 流式合成已中止".into()));
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => break, // EOF：服务端收流
            Ok(n) => {
                let aligned = align_bytes(&mut carry, &buf[..n]);
                if aligned.is_empty() {
                    continue; // 只读到 1 字节这类情况：等下一块拼齐再交
                }
                got_audio = true;
                if tx.send(StreamItem::Chunk(aligned)).is_err() {
                    return; // 接收端已 drop（abort），静默退出
                }
            }
            Err(e) => {
                if stop.load(Ordering::Relaxed) {
                    return; // 中止导致的读中断，不报假错
                }
                let _ = tx.send(StreamItem::Fail(format!("MOSS 流式读取失败: {e}")));
                return;
            }
        }
    }

    // carry 里若还剩 1 字节：半个采样点，播出来是爆音，丢弃（正常流不会走到这）
    if !got_audio {
        let _ = tx.send(StreamItem::Fail(
            "MOSS 流式未返回音频（文本可能为空或全是符号）".into(),
        ));
        return;
    }
    let _ = tx.send(StreamItem::Done);
}

// ── 注册表 ────────────────────────────────────────────────

/// 会话注册表：句柄是 u64（跨 FFI 传 `VaStreamSession` 时用 `id as *mut c_void`）
pub struct SessionRegistry {
    map: Mutex<HashMap<u64, Arc<StreamSession>>>,
    next_id: AtomicU64,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// 注册新会话并返回句柄
    pub fn insert(&self, session: StreamSession) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
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
    use crate::client::FALLBACK_CHANNELS;
    use std::io::Cursor;

    fn params() -> AudioParams {
        AudioParams {
            sample_rate: 24000,
            channels: FALLBACK_CHANNELS,
            from_headers: true,
        }
    }

    #[test]
    fn 注册表_句柄自增且终态自动摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let id = reg.insert(StreamSession::from_receiver(rx, params()));
        assert_eq!(id, 1);
        let id2 = reg.insert(StreamSession::from_receiver(channel::<StreamItem>().1, params()));
        assert_eq!(id2, 2);
        tx.send(StreamItem::Done).unwrap();
        let got = reg.next_chunk(id).unwrap();
        assert!(got.is_empty(), "Done 应返回空 vec 表示流正常结束");
        assert_eq!(reg.next_chunk(id).unwrap_err(), "流式会话不存在或已停止");
    }

    #[test]
    fn 注册表_失败透出中文原因并摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let id = reg.insert(StreamSession::from_receiver(rx, params()));
        tx.send(StreamItem::Fail("MOSS 流式失败".into())).unwrap();
        assert_eq!(reg.next_chunk(id).unwrap_err(), "MOSS 流式失败");
        assert!(reg.next_chunk(id).is_err(), "终态后应摘除会话");
    }

    #[test]
    fn abort_置位后轮询亚秒返回错误() {
        let (_tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx, params());
        session.abort();
        let t = Instant::now();
        let err = session.next_chunk().unwrap_err();
        assert!(err.contains("中止"), "实际: {err}");
        assert!(t.elapsed() < Duration::from_millis(500), "abort 应亚秒生效，实际 {:?}", t.elapsed());
    }

    /// 拉干一条会话：返回（块数, 首块耗时 ms, 总耗时 ms, 累计 PCM）
    fn drain(session: &StreamSession) -> (usize, u64, u64, Vec<u8>) {
        let t0 = Instant::now();
        let mut chunks = 0usize;
        let mut first_ms = 0u64;
        let mut pcm = Vec::<u8>::new();
        loop {
            match session.next_chunk() {
                Ok(b) if b.is_empty() => break,
                Ok(b) => {
                    if chunks == 0 {
                        first_ms = t0.elapsed().as_millis() as u64;
                    }
                    chunks += 1;
                    pcm.extend_from_slice(&b);
                }
                Err(e) => panic!("流式拉块失败: {e}"),
            }
        }
        (chunks, first_ms, t0.elapsed().as_millis() as u64, pcm)
    }

    #[test]
    fn worker_读完整段并发结束_块恒偶数字节() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5001).collect(); // 奇数长度，逼出对齐逻辑
        let (tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx, params());
        let stop = Arc::new(AtomicBool::new(false));
        // 用线程跑 worker，主线程经会话拉块（与真实路径同一套代码）
        let for_worker = data.clone();
        std::thread::spawn(move || worker_run(stop, tx, Cursor::new(for_worker)));
        let (chunks, _first, _total, pcm) = drain(&session);
        assert!(chunks >= 1, "应至少拿到一块");
        assert_eq!(pcm.len(), 5000, "奇数尾巴的最后一个字节被丢弃（半个采样点）");
        assert_eq!(pcm[..], data[..5000]);
    }

    #[test]
    fn worker_空响应报未返回音频() {
        let (tx, rx) = channel::<StreamItem>();
        worker_run(Arc::new(AtomicBool::new(false)), tx, Cursor::new(Vec::new()));
        match rx.recv().unwrap() {
            StreamItem::Fail(e) => assert!(e.contains("未返回音频"), "实际: {e}"),
            other => panic!("空响应应报失败，实际: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn worker_读错误上报原始信息() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timed out"))
            }
        }
        let (tx, rx) = channel::<StreamItem>();
        worker_run(Arc::new(AtomicBool::new(false)), tx, Failing);
        match rx.recv().unwrap() {
            StreamItem::Fail(e) => assert!(e.contains("timed out"), "实际: {e}"),
            _ => panic!("应上报 Fail"),
        }
    }

    #[test]
    fn worker_中止后不报假错() {
        let stop = Arc::new(AtomicBool::new(true)); // 进循环即中止
        let (tx, rx) = channel::<StreamItem>();
        worker_run(Arc::clone(&stop), tx, Cursor::new(vec![7u8; 100]));
        match rx.recv().unwrap() {
            StreamItem::Fail(e) => assert!(e.contains("中止"), "实际: {e}"),
            _ => panic!("中止应报中止，而不是别的"),
        }
    }

    // ── 实网探针（需真实 Key，不进常规门禁）────────────────

    /// 计时数字必须自带 profile：debug 下 rustls 慢一到两个量级，
    /// 不标 profile 的绝对值事后判断不了它能不能支撑超时决策（同 mimo 探针的口径）。
    fn profile() -> &'static str {
        if cfg!(debug_assertions) { "debug" } else { "release" }
    }

    fn pcm_ms(pcm: &[u8], p: &AudioParams) -> u64 {
        let bytes_per_second = p.sample_rate as u64 * 2 * p.channels as u64;
        pcm.len() as u64 * 1000 / bytes_per_second.max(1)
    }

    /// 把探针 PCM 封成 WAV 落到临时目录，供人耳试听（音调不对=采样率解析错了）
    fn dump_probe_wav(pcm: &[u8], p: &AudioParams) {
        let path = std::env::temp_dir().join("moss-probe-stream.wav");
        let rate = p.sample_rate;
        let channels = p.channels as u16;
        let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(pcm.len() + 44);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // 1 = PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(pcm);
        match std::fs::write(&path, &out) {
            // allowlog: 探针试听文件路径（%TEMP%），不含 Key/文本/音频内容
            Ok(_) => println!("[probe] 试听文件: {}", path.display()),
            // allowlog: 只打写入错误本身；写失败不影响断言
            Err(e) => println!("[probe] 试听文件写入失败（不影响断言）: {e}"),
        }
    }

    /// 实网探针：流式应为**多块**、音频参数应尽量来自响应头、首块显著快于整段合成。
    /// 跑法：`plugins/scripts/test-moss-stream.ps1`（脚本自己从本机 settings.json 读 Key，不打印）。
    /// 默认跑 release（交付的 dll 就是 release 构建）。
    #[test]
    #[ignore = "需真实 MOSS_TTS_API_KEY + MOSS_TTS_VOICE_ID，手动运行"]
    fn 实网探针_流式应多块且参数来自响应头() {
        let key = std::env::var("MOSS_TTS_API_KEY").expect("请先设置 MOSS_TTS_API_KEY");
        let voice = std::env::var("MOSS_TTS_VOICE_ID")
            .expect("请先设置 MOSS_TTS_VOICE_ID（Mossland 音色库复制的 voice_id）");
        let text = std::env::var("MOSS_PROBE_TEXT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "你好，这是一段流式合成的探针。今天天气不错，适合出门散步，我们边走边聊几句。".to_string());
        let model = std::env::var("MOSS_TTS_MODEL").ok();
        let req = crate::client::resolve_request(&text, Some(&voice), Some(key), model.as_deref())
            .expect("请求材料构造失败");
        let body = crate::client::build_stream_request_body(&req, &text);
        let api_key = req.api_key.clone();
        let session = StreamSession::open(api_key, body).expect("打开流式会话失败");
        let (chunks, first_ms, total_ms, pcm) = drain(&session);
        let p = session.params;
        // allowlog: 只打 profile/字数/耗时/块数/音频参数；不含 Key 与待合成文本
        println!(
            "[probe] profile {} / 字数 {} / 首块 {first_ms}ms / 块数 {chunks} / 音频 {}ms / 总耗时 {total_ms}ms / 采样率 {} / 声道 {} / 来自响应头 {}",
            profile(),
            text.chars().count(),
            pcm_ms(&pcm, &p),
            p.sample_rate,
            p.channels,
            p.from_headers
        );
        assert!(chunks > 1, "只拿到一块：服务端可能没真流式，那流式就没收益（持续如此需重估）");
        assert_eq!(pcm.len() % 2, 0, "PCM 必须 2 字节对齐");
        assert!(!pcm.starts_with(b"RIFF"), "流式块不应带 WAV 容器头");
        assert!(p.from_headers, "音频参数走了回落值——响应头里没有采样率，需按真值改 FALLBACK_SAMPLE_RATE");
        dump_probe_wav(&pcm, &p);
    }
}
