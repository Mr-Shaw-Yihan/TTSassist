// Fish Audio TTS 插件 — 调用 api.fish.audio 云端语音合成（S2.1 系列）。
//
// 合成模式：流式唯一。
//   - 流式 va_tts_stream_*：POST /v1/tts format=pcm 的分块响应边收边播——
//     官方文档明确「已握全文时低延迟首选 HTTP streaming」（WS tts-live 是给
//     LLM 增量出字场景的）；宿主流式管线只认 pcm_s16le 块，正合 HTTP 分块 PCM。
//   - 阻塞式 va_tts_synthesize（MP3 整段）保留为实现宿主加载校验所必需的
//     8 符号导出（缺一不可），作为异常路径兜底存在；不提供配置开关，
//     宿主对本插件默认走流式（plugin_config 无 "stream" 声明 → 缺省流式）。
//
// 环境变量（由宿主通用插件配置机制按 manifest.config 声明注入）：
//   FISH_AUDIO_API_KEY        必填，fish.audio → Settings → API Keys 创建
//   FISH_AUDIO_MODEL          可选，默认 s2.1-pro-free（免费）；可选 s2.1-pro / s2-pro / s1
//   FISH_AUDIO_LATENCY        可选，normal（默认）/ balanced / low
//   FISH_AUDIO_VOICE_OVERRIDE 可选，音色模型 ID（reference_id），填写后覆盖下拉选择的音色
//   FISH_AUDIO_PROXY          可选，如 http://127.0.0.1:7897；不填则依次探测
//                             环境变量代理 → Windows 系统代理，都无则直连
//
// 输出：阻塞 MP3 44.1kHz 128kbps；流式 PCM s16le 44.1kHz 单声道。
// 音色 = Fish Audio 社区模型库的 reference_id；不选音色（NULL/"default"）用官方默认音色。

use serde::Serialize;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

plugin_api::va_tts_plugin! {
    id: "fish-audio-tts",
    name: "Fish Audio TTS",
    version: "0.1.0",
    audio_format: "mp3",
    voices: voices_list,
    synthesize: synthesize,
}

plugin_api::va_tts_stream_plugin! {
    start: stream_start,
    next: stream_next,
    stop: stream_stop,
}

/// API 端点前缀
const BASE_URL: &str = "https://api.fish.audio";
/// 默认模型：s2.1-pro-free 为官方免费档（$0.00），无余额账户也可用
const DEFAULT_MODEL: &str = "s2.1-pro-free";
/// 合法 latency 档位（实测 normal/balanced/low 三值均 200）
const LATENCY_CHOICES: [&str; 3] = ["normal", "balanced", "low"];
/// 阻塞合成的单请求整体超时。宿主默认合成超时 60s，留出余量让插件先报出可读错误
const REQUEST_TIMEOUT: Duration = Duration::from_secs(50);

// ── 流式常量 ──
/// 流式响应块读取缓冲（16KB，够宿主即时起播）
const STREAM_READ_BUF: usize = 16 * 1024;
/// 流式分块粒度（字符）：服务端合成完一块即下发。取允许的最小值 100 换最快首响
/// （实测默认 300 时短文本一次到位、长文本也要等 300 字成段；100 字即发一段）
const STREAM_CHUNK_LENGTH: u32 = 100;
/// 流式 PCM 采样率（44.1kHz s16le 单声道；向宿主 info 一致声明）
const STREAM_SAMPLE_RATE: u32 = 44100;
/// 流中空闲兜底：连续无块超过该时长视为服务卡死（宿主侧注释同款 30 秒约定）
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// next 轮询周期（决定 abort/空闲判定生效时延上限）
const STREAM_POLL: Duration = Duration::from_millis(100);
/// 流式 socket 读超时：abort 后阻塞中的读取最迟此时限醒来退出
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(20);
/// 流式连接超时（宿主对流式会话 start 包装 30 秒）
const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// 音色下拉表：default + 13 个实测可用的社区音色（2026-10-06 用真实 Key 逐个验证）。
/// 挑选原则：中立风格（新闻/叙事/温柔/纪录片），不收名人克隆与梗音色。
const VOICES_JSON: &str = r#"[
{"id":"default","label":"默认音色（Fish Audio 官方）"},
{"id":"faccba1a8ac54016bcfc02761285e67f","label":"温柔动听女声"},
{"id":"57eab548c7ed4ddc974c4c153cb015b2","label":"女主播（新闻女声）"},
{"id":"59cb5986671546eaa6ca8ae6f29f6d22","label":"央视配音（新闻男声）"},
{"id":"2926cb350f1a426d800bf8c360c3cb94","label":"新闻男声"},
{"id":"23632847285a487d8e0c6ae5bc593c71","label":"温柔叙事女声"},
{"id":"32ea6ed2f793439da52685ac910534ac","label":"温柔女教师"},
{"id":"5c353fdb312f4888836a9a5680099ef0","label":"女大学生（自然女声）"},
{"id":"7f92f8afb8ec43bf81429cc1c9199cb1","label":"AD学姐（活泼女声）"},
{"id":"d99547e2dad64ce0aa085319a3c9cc56","label":"温柔男声（讲故事）"},
{"id":"7d4cc998f68c413ba5605d892d7acc87","label":"纪录片男声"},
{"id":"dd43b30d04d9446a94ebe41f301229b5","label":"宣传片男声（大气浑厚）"},
{"id":"933563129e564b19a115bedd57b7406a","label":"Sarah（英文女声）"},
{"id":"802e3bc2b27e49c2995d23ef70e6ac89","label":"Energetic Male（英文男声）"}
]"#;

/// 动态音色表函数（宿主每次查询都会调用，必须廉价且不 panic）
fn voices_list() -> Vec<plugin_api::VoiceItem> {
    serde_json::from_str(VOICES_JSON).unwrap_or_default()
}

// ── 公共配置解析（阻塞与流式同一口径）────────────────────

/// 一次合成请求用到的配置
struct ApiConfig {
    api_key: String,
    model: String,
    latency: &'static str,
    reference_id: Option<String>,
}

/// 读环境变量组装配置（错误均为面向用户的中文）
fn read_config(text: &str, voice: Option<&str>) -> Result<ApiConfig, String> {
    if text.trim().is_empty() {
        return Err("合成文本为空".to_string());
    }
    let api_key = std::env::var("FISH_AUDIO_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            "未设置 Fish Audio API Key，请在设置页的「API 密钥」卡中填写。\n\
             获取地址：https://fish.audio （登录后 Settings → API Keys）"
                .to_string()
        })?;
    let model = std::env::var("FISH_AUDIO_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let latency_raw = std::env::var("FISH_AUDIO_LATENCY").ok();
    let latency = resolve_latency_with(latency_raw.as_deref())?;
    let override_id = std::env::var("FISH_AUDIO_VOICE_OVERRIDE").ok();
    let reference_id = resolve_reference_id_with(voice, override_id.as_deref());
    Ok(ApiConfig {
        api_key: api_key.trim().to_string(),
        model,
        latency,
        reference_id,
    })
}

/// 音色 → reference_id 的纯函数核心（覆盖值由调用方读好传入）。
/// 规则：覆盖值有效 → 用它（填 "default" = 显式要官方默认）；
/// 覆盖空白/未配置 → 按下拉选择，未选或选了 default → None（官方默认音色）。
fn resolve_reference_id_with(voice: Option<&str>, override_id: Option<&str>) -> Option<String> {
    if let Some(id) = override_id {
        let id = id.trim();
        if !id.is_empty() {
            return if id == "default" { None } else { Some(id.to_string()) };
        }
    }
    match voice {
        None | Some("") | Some("default") => None,
        Some(v) => Some(v.to_string()),
    }
}

/// latency 解析：空值用默认档；非法值直接报错（不静默兜底，
/// 与「非法入参一律报错」的仓库口径一致，避免用户配错却听不出原因）
fn resolve_latency_with(raw: Option<&str>) -> Result<&'static str, String> {
    match raw.map(str::trim) {
        None | Some("") => Ok("normal"),
        Some(v) if LATENCY_CHOICES.contains(&v) => {
            Ok(LATENCY_CHOICES.iter().find(|c| **c == v).copied().unwrap())
        }
        Some(v) => Err(format!(
            "FISH_AUDIO_LATENCY 取值非法：{v}（合法值：normal / balanced / low）"
        )),
    }
}

// ── HTTP 层 ───────────────────────────────────────────────

/// 组装带代理探测的 AgentBuilder（阻塞与流式共用）
fn agent_builder() -> ureq::AgentBuilder {
    let mut builder = ureq::AgentBuilder::new();
    if let Some(p) = resolve_proxy() {
        if let Ok(proxy) = ureq::Proxy::new(&p) {
            builder = builder.proxy(proxy);
        }
    }
    builder
}

/// POST /v1/tts 通用发送：Ok=响应头已到达（body 交给调用方读）；
/// 非 2xx / 网络错原样带给调用方处理。
fn post_tts(
    agent: &ureq::Agent,
    cfg: &ApiConfig,
    req: &impl Serialize,
) -> Result<ureq::Response, ureq::Error> {
    agent
        .post(&format!("{BASE_URL}/v1/tts"))
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("model", &cfg.model)
        .send_json(req)
}

/// 非 2xx 响应 → 面向用户的中文错误（错误体统一 {"status","message"}，
/// 422 是校验对象数组；两种形态均实测确认）
fn friendly_error(code: u16, body: &str) -> String {
    let api_msg = parse_error_body(body);
    match code {
        401 => format!(
            "Fish Audio API Key 无效或已过期（401{}），请检查设置页的 API 密钥",
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
        402 => "Fish Audio 账户余额不足（402），当前模型需要付费；可在配置中把模型切换为免费档 s2.1-pro-free（突发连续调用也可能短暂 402，稍后自动恢复）".to_string(),
        429 => format!(
            "Fish Audio 请求过于频繁（429{}），请稍后重试",
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
        _ => format!(
            "Fish Audio 合成失败（HTTP {}{}）",
            code,
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
    }
}

/// 从错误响应体提取 message（对象 {"status","message"} 或数组 [{loc,msg,...}]）
fn parse_error_body(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v {
        serde_json::Value::Object(map) => map
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        serde_json::Value::Array(items) => {
            let msgs: Vec<String> = items
                .iter()
                .filter_map(|it| it.get("msg").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .collect();
            if msgs.is_empty() { None } else { Some(msgs.join("；")) }
        }
        _ => None,
    }
}

// ── 阻塞合成 ──────────────────────────────────────────────

/// POST /v1/tts 阻塞请求体。字段名按官方 OpenAPI 为 snake_case；
/// reference_id 缺省时服务端用官方默认音色，故 None 时省略该键。
#[derive(Serialize)]
struct TtsRequest<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_id: Option<String>,
    format: &'a str,
    latency: &'a str,
}

/// 文本 → MP3 字节（voice 为 None 或 "default" 用官方默认音色）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let cfg = read_config(text, voice)?;
    let req = TtsRequest {
        text,
        reference_id: cfg.reference_id.clone(),
        format: "mp3",
        latency: cfg.latency,
    };

    let agent = agent_builder().timeout(REQUEST_TIMEOUT).build();
    match post_tts(&agent, &cfg, &req) {
        Ok(r) => {
            // ureq 2.x 无 into_bytes，读 reader 收全量音频（分块响应统一缓冲）
            let mut reader = r.into_reader();
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Fish Audio 音频流读取失败: {e}"))?;
            if bytes.is_empty() {
                return Err("Fish Audio 返回空音频".to_string());
            }
            Ok(bytes)
        }
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            Err(friendly_error(code, &body))
        }
        Err(e) => Err(format!(
            "Fish Audio 网络请求失败: {e}（若网络需代理，请在配置中填写代理地址）"
        )),
    }
}

// ── 流式合成（HTTP chunked PCM）──────────────────────────

/// 流式请求体：format=pcm + 显式采样率 + 最小分块粒度（换最快首响）
#[derive(Serialize)]
struct StreamRequest<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_id: Option<String>,
    format: &'a str,
    latency: &'a str,
    sample_rate: u32,
    chunk_length: u32,
}

fn stream_info_json() -> String {
    format!(r#"{{"format":"pcm_s16le","sample_rate":{STREAM_SAMPLE_RATE},"channels":1}}"#)
}

/// 宿主拉块通道的载荷
enum StreamItem {
    Chunk(Vec<u8>),
    Done,
    Failed(String),
}

/// 会话终态（缓存后重复 next 返回同一结果）
#[derive(Clone)]
enum StreamOutcome {
    Ended,
    Failed(String),
}

/// 一次流式会话：后台线程读 HTTP 分块响应 → mpsc；next 端轮询取块。
/// abort 只置停止位并移出注册表（rx 随之 drop，读线程发送失败自退）；
/// 阻塞中的 next 靠轮询周期亚秒级返回错误（宿主 stop 可从任意线程调用）。
struct FishStreamSession {
    rx: Mutex<Receiver<StreamItem>>,
    /// 与读线程共享的停止位
    aborted: Arc<AtomicBool>,
    /// 最近一次收到块的时机（空闲兜底判定）
    last_progress: Mutex<Instant>,
    terminal: Mutex<Option<StreamOutcome>>,
}

impl FishStreamSession {
    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }
    fn set_terminal(&self, outcome: StreamOutcome) {
        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
    }
    fn terminal_cached(&self) -> Option<StreamOutcome> {
        self.terminal.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn touch(&self) {
        *self.last_progress.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }
    fn idle_expired(&self) -> bool {
        self.last_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
            >= STREAM_IDLE_TIMEOUT
    }
}

/// 打开流式会话：发请求（连接/鉴权/4xx 在 start 即报错——宿主把 start 失败视为
/// 「打开流式会话失败」干净回退提示，不留给拉块循环）、成功则起后台读线程。
fn stream_start(text: &str, voice: Option<&str>) -> Result<(String, u64), String> {
    let cfg = read_config(text, voice)?;
    let req = StreamRequest {
        text,
        reference_id: cfg.reference_id.clone(),
        format: "pcm",
        latency: cfg.latency,
        sample_rate: STREAM_SAMPLE_RATE,
        chunk_length: STREAM_CHUNK_LENGTH,
    };

    let agent = agent_builder()
        .timeout_connect(STREAM_CONNECT_TIMEOUT)
        .timeout_read(STREAM_READ_TIMEOUT)
        .build();
    let resp = match post_tts(&agent, &cfg, &req) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            return Err(friendly_error(code, &body));
        }
        Err(e) => {
            return Err(format!(
                "Fish Audio 流式请求失败: {e}（若网络需代理，请在配置中填写代理地址）"
            ))
        }
    };

    let reader = resp.into_reader(); // 'static 拥有连接，agent 可先行析构
    let (tx, rx) = channel();
    let aborted = Arc::new(AtomicBool::new(false));
    let abort_for_thread = Arc::clone(&aborted);
    std::thread::Builder::new()
        .name("fish-audio-stream".into())
        .spawn(move || stream_reader_loop(reader, tx, abort_for_thread))
        .map_err(|e| format!("启动流式读取线程失败: {e}"))?;

    let session = Arc::new(FishStreamSession {
        rx: Mutex::new(rx),
        aborted,
        last_progress: Mutex::new(Instant::now()),
        terminal: Mutex::new(None),
    });
    let id = global_sessions().insert(session);
    Ok((stream_info_json(), id))
}

/// 后台读线程主体：把 reader 分块读成偶数字节段推入 tx；EOF→Done；
/// 错误→Failed（abort 后静默退出，不报假错）。
fn stream_reader_loop(mut reader: impl Read, tx: Sender<StreamItem>, aborted: Arc<AtomicBool>) {
    let mut buf = vec![0u8; STREAM_READ_BUF];
    let mut pending: Vec<u8> = Vec::new(); // 奇数尾巴跨次拼接，保证块 2 字节对齐
    loop {
        if aborted.load(Ordering::Relaxed) {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF：尾巴残余凑不成帧就丢弃；够一帧先发出去再报结束
                if !pending.is_empty() {
                    let even = pending.len() & !1;
                    if even > 0 {
                        let tail: Vec<u8> = pending.drain(..even).collect();
                        if tx.send(StreamItem::Chunk(tail)).is_err() {
                            return;
                        }
                    }
                }
                let _ = tx.send(StreamItem::Done);
                return;
            }
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                let even = pending.len() & !1;
                if even > 0 {
                    let chunk: Vec<u8> = pending.drain(..even).collect();
                    if tx.send(StreamItem::Chunk(chunk)).is_err() {
                        return; // 接收端已 drop（abort），静默退出
                    }
                }
            }
            Err(e) => {
                if aborted.load(Ordering::Relaxed) {
                    return;
                }
                let _ = tx.send(StreamItem::Failed(format!(
                    "Fish Audio 流式音频读取失败: {e}"
                )));
                return;
            }
        }
    }
}

/// 拉取下一块。Ok(空 vec)=流正常结束；Err=失败/中止/超时。
fn stream_next(id: u64) -> Result<Vec<u8>, String> {
    let session = match global_sessions().get(id) {
        Some(s) => s,
        None => return Err("流式会话不存在或已结束".to_string()),
    };
    if session.is_aborted() {
        return Err("流式合成已中止".to_string());
    }
    if let Some(t) = session.terminal_cached() {
        return match t {
            StreamOutcome::Ended => Ok(Vec::new()),
            StreamOutcome::Failed(e) => Err(e),
        };
    }
    loop {
        let item = {
            let rx = session.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(STREAM_POLL)
        };
        match item {
            Ok(StreamItem::Chunk(b)) => {
                session.touch();
                return Ok(b);
            }
            Ok(StreamItem::Done) => {
                session.set_terminal(StreamOutcome::Ended);
                return Ok(Vec::new());
            }
            Ok(StreamItem::Failed(e)) => {
                session.set_terminal(StreamOutcome::Failed(e.clone()));
                return Err(e);
            }
            Err(RecvTimeoutError::Timeout) => {
                if session.is_aborted() {
                    return Err("流式合成已中止".to_string());
                }
                if session.idle_expired() {
                    let e = format!(
                        "Fish Audio 流式合成无响应（{} 秒未收到新音频块）",
                        STREAM_IDLE_TIMEOUT.as_secs()
                    );
                    session.set_terminal(StreamOutcome::Failed(e.clone()));
                    return Err(e);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                // 读线程异常退出（abort 移除注册表后 rx drop，或线程崩溃）
                session.set_terminal(StreamOutcome::Failed("流式合成中断".to_string()));
                return Err("流式合成中断".to_string());
            }
        }
    }
}

/// 中止并释放会话（幂等，任意线程可调）：置停止位 + 移出注册表。
/// 拉块循环亚秒级返回中止错误；读线程随 rx drop 自退。
fn stream_stop(id: u64) {
    global_sessions().abort_and_remove(id);
}

// ── 会话注册表 ────────────────────────────────────────────

struct SessionRegistry {
    map: Mutex<std::collections::HashMap<u64, Arc<FishStreamSession>>>,
    next_id: AtomicU64,
}

impl SessionRegistry {
    fn new() -> Self {
        Self {
            map: Mutex::new(std::collections::HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn insert(&self, session: Arc<FishStreamSession>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, session);
        id
    }

    fn get(&self, id: u64) -> Option<Arc<FishStreamSession>> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    fn abort_and_remove(&self, id: u64) {
        let removed = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if let Some(s) = removed {
            s.aborted.store(true, Ordering::Relaxed);
        }
    }
}

fn global_sessions() -> &'static SessionRegistry {
    static REG: OnceLock<SessionRegistry> = OnceLock::new();
    REG.get_or_init(SessionRegistry::new)
}

// ── 代理探测（api.fish.audio 国内直连常超时，代理是刚需）──
//
// 优先级：FISH_AUDIO_PROXY 显式配置 → 环境变量（HTTPS_PROXY/ALL_PROXY 等）
// → Windows 系统代理（注册表 ProxyEnable/ProxyServer）→ 直连。

/// 代理解析纯函数核心（各来源按优先级取第一个非空值；纯空白视同未配置）
fn pick_proxy<'a>(
    explicit: Option<&'a str>,
    env_https: Option<&'a str>,
    env_all: Option<&'a str>,
    system: Option<String>,
) -> Option<String> {
    let from_str = |s: Option<&'a str>| -> Option<String> {
        s.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(normalize_proxy_url)
    };
    from_str(explicit)
        .or_else(|| from_str(env_https))
        .or_else(|| from_str(env_all))
        .or(system)
}

/// 实际代理解析：显式配置 → 环境变量 → Windows 系统代理
fn resolve_proxy() -> Option<String> {
    let explicit = std::env::var("FISH_AUDIO_PROXY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let env_https = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    let env_all = std::env::var("ALL_PROXY")
        .or_else(|_| std::env::var("all_proxy"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    pick_proxy(
        explicit.as_deref(),
        env_https.as_deref(),
        env_all.as_deref(),
        windows_system_proxy(),
    )
}

/// 补全代理 URL 的 scheme（注册表/环境变量常见 "127.0.0.1:7897" 裸地址）
fn normalize_proxy_url(raw: &str) -> String {
    let s = raw.trim();
    if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// 读 Windows 系统代理（HKCU Internet Settings），非 Windows 一律 None
#[cfg(windows)]
fn windows_system_proxy() -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    parse_windows_proxy_server(&server)
}

#[cfg(not(windows))]
fn windows_system_proxy() -> Option<String> {
    None
}

/// 解析注册表 ProxyServer 的三种实测取值形态：
///   "127.0.0.1:7897"                          → 全协议同一地址
///   "127.0.0.1:7897;localhost:7897"（罕见）    → 取第一段
///   "http=…;https=…;ftp=…"（分协议）           → 优先 https，其次 http
fn parse_windows_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        let mut https = None;
        let mut http = None;
        for part in server.split(';') {
            let part = part.trim();
            if let Some((scheme, addr)) = part.split_once('=') {
                match scheme.trim().to_ascii_lowercase().as_str() {
                    "https" => https = Some(addr.trim().to_string()),
                    "http" => http = Some(addr.trim().to_string()),
                    _ => {}
                }
            }
        }
        https.or(http).map(|addr| normalize_proxy_url(&addr))
    } else {
        let first = server.split(';').next().unwrap_or(server).trim();
        if first.is_empty() { None } else { Some(normalize_proxy_url(first)) }
    }
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn 音色表是合法json且条目齐全() {
        let v: Vec<serde_json::Value> = serde_json::from_str(VOICES_JSON).unwrap();
        assert!(v.len() >= 14);
        assert_eq!(v[0]["id"], "default");
        for item in &v[1..] {
            let id = item["id"].as_str().unwrap();
            assert_eq!(id.len(), 32, "音色 id 长度应为 32：{id}");
            assert!(
                id.chars().all(|c| c.is_ascii_hexdigit()),
                "音色 id 应为十六进制：{id}"
            );
        }
        for item in &v {
            assert!(!item["label"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn 阻塞请求体_默认音色时省略reference_id() {
        let req = TtsRequest {
            text: "测试",
            reference_id: None,
            format: "mp3",
            latency: "normal",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("reference_id").is_none(), "默认音色不应携带 reference_id 键");
        assert_eq!(json["text"], "测试");
    }

    #[test]
    fn 流式请求体声明pcm采样率与分块粒度() {
        let req = StreamRequest {
            text: "测试",
            reference_id: Some("abc".into()),
            format: "pcm",
            latency: "normal",
            sample_rate: 44100,
            chunk_length: 100,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["format"], "pcm");
        assert_eq!(json["sample_rate"], 44100);
        assert_eq!(json["chunk_length"], 100);
        assert_eq!(json["reference_id"], "abc");
    }

    #[test]
    fn 流式info_json符合宿主pcm_s16le约定() {
        let info: serde_json::Value = serde_json::from_str(&stream_info_json()).unwrap();
        assert_eq!(info["format"], "pcm_s16le");
        assert_eq!(info["sample_rate"], STREAM_SAMPLE_RATE);
        assert_eq!(info["channels"], 1);
    }

    #[test]
    fn 音色解析_未选与default都用官方默认() {
        assert_eq!(resolve_reference_id_with(None, None), None);
        assert_eq!(resolve_reference_id_with(Some("default"), None), None);
        assert_eq!(resolve_reference_id_with(Some(""), None), None);
        // 覆盖值填 default 也算官方默认（容错：用户可能照抄下拉项）
        assert_eq!(resolve_reference_id_with(Some("abc"), Some("default")), None);
    }

    #[test]
    fn 音色解析_普通音色原样透传() {
        let id = "59cb5986671546eaa6ca8ae6f29f6d22";
        assert_eq!(
            resolve_reference_id_with(Some(id), None),
            Some(id.to_string())
        );
    }

    #[test]
    fn 音色解析_配置覆盖优先于下拉选择() {
        assert_eq!(
            resolve_reference_id_with(
                Some("59cb5986671546eaa6ca8ae6f29f6d22"),
                Some(" faccba1a8ac54016bcfc02761285e67f ")
            ),
            Some("faccba1a8ac54016bcfc02761285e67f".to_string())
        );
        // 覆盖值为空白时不算配置，回落到下拉选择
        assert_eq!(
            resolve_reference_id_with(Some("abc"), Some("   ")),
            Some("abc".to_string())
        );
        // 覆盖空白且未选 → 官方默认
        assert_eq!(resolve_reference_id_with(None, Some("")), None);
    }

    #[test]
    fn latency_空值用默认_非法值报错() {
        assert_eq!(resolve_latency_with(None).unwrap(), "normal");
        assert_eq!(resolve_latency_with(Some("")).unwrap(), "normal");
        assert_eq!(resolve_latency_with(Some(" balanced ")).unwrap(), "balanced");
        assert_eq!(resolve_latency_with(Some("low")).unwrap(), "low");
        assert!(resolve_latency_with(Some("fast")).is_err());
    }

    #[test]
    fn 错误体解析_对象与数组两种形态() {
        assert_eq!(
            parse_error_body(r#"{"status":401,"message":"Invalid Token"}"#).as_deref(),
            Some("Invalid Token")
        );
        let body = r#"[{"type":"literal_error","loc":["latency"],"msg":"bad value"}]"#;
        assert_eq!(parse_error_body(body).as_deref(), Some("bad value"));
        assert_eq!(parse_error_body("not json"), None);
    }

    #[test]
    fn 中文错误映射_常见状态码() {
        let e401 = friendly_error(401, r#"{"status":401,"message":"Invalid Token"}"#);
        assert!(e401.contains("API Key 无效"), "{e401}");
        assert!(e401.contains("Invalid Token"), "{e401}");
        assert!(friendly_error(402, "{}").contains("余额不足"));
        assert!(friendly_error(429, "{}").contains("频繁"));
        let e400 = friendly_error(400, r#"{"status":400,"message":"oops"}"#);
        assert!(e400.contains("HTTP 400") && e400.contains("oops"), "{e400}");
    }

    #[test]
    fn 代理解析_优先级与环境变量格式() {
        assert_eq!(
            pick_proxy(
                Some("127.0.0.1:7897"),
                Some("http://a:1"),
                None,
                Some("http://s:1".into())
            )
            .as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            pick_proxy(None, Some("http://a:1"), None, Some("http://s:1".into())).as_deref(),
            Some("http://a:1")
        );
        assert_eq!(
            pick_proxy(None, None, Some("socks5://b:2"), Some("http://s:1".into())).as_deref(),
            Some("socks5://b:2")
        );
        assert_eq!(
            pick_proxy(None, None, None, Some("http://s:1".into())).as_deref(),
            Some("http://s:1")
        );
        assert_eq!(pick_proxy(None, None, None, None), None);
        // 空串与纯空白视同未配置
        assert_eq!(pick_proxy(Some("  "), None, None, None), None);
    }

    #[test]
    fn 代理解析_注册表服务器三种形态() {
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            parse_windows_proxy_server("http=10.0.0.1:8080;https=10.0.0.2:8443;ftp=10.0.0.3:21")
                .as_deref(),
            Some("http://10.0.0.2:8443")
        );
        assert_eq!(
            parse_windows_proxy_server("http=10.0.0.1:8080").as_deref(),
            Some("http://10.0.0.1:8080")
        );
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897;localhost:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(parse_windows_proxy_server(""), None);
        assert_eq!(parse_windows_proxy_server("   "), None);
    }

    // ── 流式语义测试（不联网，直接驱动读线程/注册表）──

    #[test]
    fn 读线程正常结束且块偶数字节对齐() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5001).collect(); // 奇数尾巴
        let reader = Cursor::new(data.clone());
        let (tx, rx) = channel();
        stream_reader_loop(reader, tx, Arc::new(AtomicBool::new(false)));
        let mut got = Vec::new();
        let mut ended = false;
        while let Ok(item) = rx.recv() {
            match item {
                StreamItem::Chunk(b) => {
                    assert_eq!(b.len() % 2, 0, "推给宿主的块必须偶数字节（s16le 帧对齐）");
                    got.extend_from_slice(&b);
                }
                StreamItem::Done => {
                    ended = true;
                    break;
                }
                StreamItem::Failed(e) => panic!("不应失败: {e}"),
            }
        }
        assert!(ended, "EOF 应产生 Done");
        assert_eq!(got.len(), 5000);
        assert_eq!(got[..], data[..5000]);
    }

    #[test]
    fn 读线程_abort后静默退出() {
        // reader 第一次 read 时置 abort——循环顶部检查会先于读退出
        struct AbortTrigger {
            inner: Cursor<Vec<u8>>,
            flag: Arc<AtomicBool>,
            fired: std::cell::Cell<bool>,
        }
        impl Read for AbortTrigger {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if !self.fired.replace(true) {
                    self.flag.store(true, Ordering::Relaxed);
                }
                self.inner.read(buf)
            }
        }
        let flag = Arc::new(AtomicBool::new(false));
        let reader = AbortTrigger {
            inner: Cursor::new(vec![7u8; 100]),
            flag: Arc::clone(&flag),
            fired: std::cell::Cell::new(false),
        };
        let (tx, rx) = channel();
        stream_reader_loop(reader, tx, Arc::clone(&flag));
        // 首块可能先入队（首次 read 才置位），关键是绝不能上报 Failed
        while let Ok(item) = rx.try_recv() {
            if let StreamItem::Failed(e) = item {
                panic!("abort 后不应报失败: {e}");
            }
        }
    }

    #[test]
    fn 读线程错误上报含原始信息() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timed out"))
            }
        }
        let (tx, rx) = channel();
        stream_reader_loop(Failing, tx, Arc::new(AtomicBool::new(false)));
        match rx.recv().unwrap() {
            StreamItem::Failed(e) => {
                assert!(e.contains("timeout") || e.contains("timed out"), "{e}")
            }
            _ => panic!("应上报 Failed"),
        }
    }

    #[test]
    fn 会话注册表_编号递增与abort移除幂等() {
        let mk = || {
            Arc::new(FishStreamSession {
                rx: Mutex::new(channel::<StreamItem>().1),
                aborted: Arc::new(AtomicBool::new(false)),
                last_progress: Mutex::new(Instant::now()),
                terminal: Mutex::new(None),
            })
        };
        let reg = SessionRegistry::new();
        let id1 = reg.insert(mk());
        let id2 = reg.insert(mk());
        assert_ne!(id1, id2);
        assert!(reg.get(id1).is_some());
        reg.abort_and_remove(id1);
        assert!(reg.get(id1).is_none(), "abort 后注册表应移除");
        assert!(!reg.get(id2).unwrap().is_aborted());
        reg.abort_and_remove(id1); // 二次 abort 幂等
        reg.abort_and_remove(9999); // 未知 id 不炸
    }

    #[test]
    fn 会话终态缓存与abort位() {
        let session = FishStreamSession {
            rx: Mutex::new(channel::<StreamItem>().1),
            aborted: Arc::new(AtomicBool::new(false)),
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        };
        assert!(session.terminal_cached().is_none());
        session.set_terminal(StreamOutcome::Failed("测试失败".into()));
        assert!(matches!(
            session.terminal_cached(),
            Some(StreamOutcome::Failed(_))
        ));
        session.set_terminal(StreamOutcome::Ended);
        assert!(matches!(session.terminal_cached(), Some(StreamOutcome::Ended)));
        assert!(!session.is_aborted());
        session.aborted.store(true, Ordering::Relaxed);
        assert!(session.is_aborted());
    }
}
