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

use crate::client::{
    align_bytes, sse_line_to_pcm, strip_wav_container, trim_for_msg, ENDPOINT,
};

/// 相邻音频块最大间隔：超过视为服务端失联（正常流式块间隔为亚秒级）
const CHUNK_GAP_TIMEOUT: Duration = Duration::from_secs(30);
/// 首块前的等待上限：兼容模式（克隆）是“全文推理完才出块”，实测 60 字要 3.5s，
/// 长句会成倍增长。拿块间的 30s 当首块预算会把正常推理误判成失联，
/// 因此首块单给一个宽松窗口（仍小于 REQUEST_TIMEOUT，死连接不会永挂）。
const FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(120);
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
    /// 是否已交出过首块（决定用首块窗口还是块间窗口）
    got_first: AtomicBool,
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
            got_first: AtomicBool::new(false),
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
            got_first: AtomicBool::new(false),
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
                        return Err("MiMo 流式合成已中止".into());
                    }
                    // 首块前用宽松窗口（兼容模式全文推理），首块后用块间窗口（亚秒级）
                    let idle = self.last_progress.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    let limit = if self.got_first.load(Ordering::Relaxed) {
                        CHUNK_GAP_TIMEOUT
                    } else {
                        FIRST_CHUNK_TIMEOUT
                    };
                    if idle > limit {
                        let msg = format!(
                            "MiMo 流式服务 {} 秒未返回音频，连接中断",
                            limit.as_secs()
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
                let _ = tx.send(StreamItem::Fail(format!(
                    "MiMo 流式读取失败: {}",
                    trim_for_msg(&e.to_string())
                )));
                return;
            }
        };
        if n == 0 {
            break; // EOF：服务端收流（BufRead::read_until 在 EOF 会带着无换行的残行返回 Ok(n>0)，
                   // 所以残行已在循环内喂过，此处不需额外补处理）
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if !feed_line_or_fail(&tx, text, &mut carry, &mut first, &mut got_audio) {
            return; // 已发出 Fail
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
    }
    let aligned = align_bytes(carry, &bytes);
    if aligned.is_empty() {
        // 还没真正交出首块（网关常先推一个空 base64 的 delta）：探测位必须保留，
        // 否则真正的 WAV 首块会逃过剥头，开头 44 字节变成爆音
        return true;
    }
    *first = false;
    *got_audio = true;
    tx.send(StreamItem::Chunk(aligned)).is_ok()
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

    /// 计时数字必须自带 profile：debug 下 rustls + 大 base64 慢很多，
    /// 不标 profile 的绝对值事后根本判断不了它能不能支撑超时决策。
    fn profile() -> &'static str {
        if cfg!(debug_assertions) { "debug" } else { "release" }
    }

    /// 拉干一条会话：返回（块数, 首块耗时ms, 总耗时ms, 首块前 4 字节, 累计 PCM）。
    /// `tag` 只用来让失败文案能看出是哪只探针（实测那行 panic 是抄进文档的唯一现场证据，
    /// 不带指向就得回头翻是哪个探针跑的）。
    /// 计时从 `open()` 返回后才起算，而 `FIRST_CHUNK_TIMEOUT` 也是从会话建立后开始计时
    /// （stream.rs 里 `last_progress` 的初值就是 `Instant::now()`），两者口径一致——
    /// 拿首块耗时判超时预算是对的；但用户实际听见的延迟还包含握手与响应头，那部分不在本表里。
    /// 两只探针的收块循环原本逐行重复，抽出来顺带统一两个口径：
    /// 首块不足 4 字节时取 `min`（不能让探针自己越界 panic 而误报成“剥头失效”），
    /// 以及“空块=终态”的约定只写一遍。
    fn drain_session(session: &StreamSession, tag: &str) -> (usize, u64, u64, [u8; 4], Vec<u8>) {
        let t0 = Instant::now();
        let mut chunks = 0usize;
        let mut first_ms = 0u64;
        let mut first_head = [0u8; 4];
        let mut pcm = Vec::<u8>::new();
        loop {
            match session.next_chunk() {
                Ok(b) if b.is_empty() => break,
                Ok(b) => {
                    if chunks == 0 {
                        first_ms = t0.elapsed().as_millis() as u64;
                        let n = b.len().min(4);
                        first_head[..n].copy_from_slice(&b[..n]);
                    }
                    chunks += 1;
                    pcm.extend_from_slice(&b);
                }
                Err(e) => panic!("{tag}流式拉块失败: {e}"),
            }
        }
        (chunks, first_ms, t0.elapsed().as_millis() as u64, first_head, pcm)
    }

    /// 按插件自述的采样率/声道数算 PCM 时长（16bit = 2 字节）
    fn pcm_ms(pcm: &[u8]) -> u64 {
        // 这个乘出来是“每秒字节数”，调用式里先 * 1000 才是 ms；
        // 名字写成 bytes_per_second 而不是 bytes_per_ms，以免被人“顺手修正”成 / 1000
        let bytes_per_second =
            crate::client::STREAM_SAMPLE_RATE as u64 * 2 * crate::client::STREAM_CHANNELS as u64;
        pcm.len() as u64 * 1000 / bytes_per_second
    }

    /// 把探针 PCM 封成 WAV 落到临时目录，供人耳试听（只手动跑，不进常规门禁）
    fn dump_probe_wav(pcm: &[u8], tag: &str) {
        let path = std::env::temp_dir().join(format!("mimo-probe-{tag}.wav"));
        let rate = crate::client::STREAM_SAMPLE_RATE;
        let channels = crate::client::STREAM_CHANNELS as u16;
        // WAV 的长度字段是 u32：探针永远够不到 4GB，但静默截断会产出“头对不上”的坏文件，
        // 不如钉在 u32::MAX，听不了就是一眼能看出来的错
        let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(pcm.len() + 44);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // 音频格式：1 = PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(pcm);
        match std::fs::write(&path, &out) {
            // allowlog: #[ignore] 探针试听文件的落盘路径（%TEMP%），不含 Key/文本/音频内容
            Ok(_) => println!("[probe] 试听文件: {}", path.display()),
            // allowlog: 只打写入错误本身；写失败不影响断言，但不能假装文件存在
            Err(e) => println!("[probe] 试听文件写入失败（不影响断言）: {e}"),
        }
    }

    /// 探针待合成文本：默认短句，可用 `MIMO_PROBE_TEXT` 临时换长句。
    /// 首块耗时跟文本长度强相关（兼容模式是全文推理完才出块），要测长句预算
    /// 不该为此改代码（改一次就忘一次）。
    fn probe_text(default: &str) -> String {
        std::env::var("MIMO_PROBE_TEXT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    /// 实网探针①：预置音色应当是"多块"真流式。
    /// 跑法：`plugins/scripts/test-mimo-stream.ps1`。默认跑 **release**（交付的 dll 就是 release 构建），
    /// 加 `-Quick` 才跑 debug——实测同一只克隆探针 release 要 1.9s、debug 能在 1.9s ~ 74s 之间乱跳
    /// （TLS/base64 在 debug 下慢一到两个量级），所以 **debug 的数字不能拿来定超时预算**；
    /// 两种 profile 的实测值都记在计划 Task 4 实施记录里（release 下预置音色首块甚至不足 1ms，
    /// 说明首块早在 `open()` 期间就到了，那部分延迟不在此计时窗口内）。
    #[test]
    #[ignore = "需真实 MIMO_TTS_API_KEY，手动运行"]
    fn 实网探针_预置音色应为多块流式() {
        let key = std::env::var("MIMO_TTS_API_KEY").expect("请先设置 MIMO_TTS_API_KEY");
        let text = probe_text("你好，这是一段流式合成的探针。今天天气不错，适合出门散步，我们边走边多聊几句。");
        let body = crate::client::build_stream_request_body(
            &crate::client::configured_model(),
            &text,
            Some(crate::client::DEFAULT_VOICE),
            None,
        );
        let session = StreamSession::open(key.trim().to_string(), body).expect("打开流式会话失败");
        let (chunks, first_ms, ms_total, _first_head, pcm) = drain_session(&session, "预置");
        // allowlog: 只打 profile/字数/耗时/块数；字数不是文本内容，不含 Key 与待合成文本
        println!(
            "[probe] 预置：profile {} / 字数 {} / 首块 {first_ms}ms / 块数 {chunks} / 音频 {}ms / 总耗时 {ms_total}ms",
            profile(),
            text.chars().count(),
            pcm_ms(&pcm)
        );
        assert!(!pcm.starts_with(b"RIFF"), "流式块不应带 WAV 容器头");
        assert_eq!(pcm.len() % 2, 0, "PCM 必须 2 字节对齐");
        assert!(chunks > 1, "预置音色未拿到多块：服务端可能退化为整段返回（持续如此需重估流式收益）");
        dump_probe_wav(&pcm, "preset");
    }

    /// 实网探针②：克隆音色（服务端兼容模式）首块必须被处理成可播 PCM。
    /// `strip_wav_container` 已在 worker 首块处生效，故这里断言"拿不到 RIFF 开头"。
    /// 需额外设置 VA_CLONE_SAMPLE 指向一段 5–10 秒人声样本。
    #[test]
    #[ignore = "需真实 MIMO_TTS_API_KEY + VA_CLONE_SAMPLE，手动运行"]
    fn 实网探针_克隆音色首块不得带容器头() {
        let key = std::env::var("MIMO_TTS_API_KEY").expect("请先设置 MIMO_TTS_API_KEY");
        let sample = std::env::var("VA_CLONE_SAMPLE").expect("请先设置 VA_CLONE_SAMPLE=样本绝对路径");
        assert!(
            std::path::Path::new(&sample).exists(),
            "VA_CLONE_SAMPLE 指向的文件不存在: {sample}"
        );
        let text = probe_text("这是一段克隆音色的流式探针，用来确认开头没有爆音。");
        let (model, voice) = crate::client::resolve_voice(Some("clone")).expect("克隆请求体构造失败");
        let body = crate::client::build_stream_request_body(&model, &text, voice.as_deref(), None);
        let session = StreamSession::open(key.trim().to_string(), body).expect("打开流式会话失败");
        // 计时必须记：兼容模式（克隆）的“首块前等待”就是 FIRST_CHUNK_TIMEOUT 能不能立住的唯一实测证据
        let (chunks, first_ms, ms_total, first_head, pcm) = drain_session(&session, "克隆");
        // allowlog: 只打 profile/字数/耗时/块数/首块前 4 字节（纯音频字节）；不含 Key 与待合成文本
        println!(
            "[probe] 克隆：profile {} / 字数 {} / 块数 {chunks} / 首块 {first_ms}ms / 音频 {}ms / 总耗时 {ms_total}ms / 首块前 4 字节 {:?}",
            profile(),
            text.chars().count(),
            pcm_ms(&pcm),
            first_head
        );
        assert!(chunks >= 1, "克隆音色未返回任何音频块");
        // 断累计结果而不是断补零后的 4 字节数组：首块只有 2 字节时后者会静默放过 "RI"
        assert!(
            !pcm.starts_with(b"RIFF"),
            "首块仍是 WAV 容器头：strip_wav_container 未生效"
        );
        assert_eq!(pcm.len() % 2, 0, "PCM 必须 2 字节对齐");
        dump_probe_wav(&pcm, "clone");
    }
}
