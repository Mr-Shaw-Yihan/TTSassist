// 流式 TTS 管线：插件流式会话（WS 逐块 PCM）→ 边收边播（扬声器 + 可选虚拟麦克风）
// → 全量 PCM 转 WAV 落盘 → 消息记录。
//
// 设计（独立于 feat/tts-streaming 分支的句内字节流方案，供效果对比）：
// - 拉模型：宿主 spawn_blocking 循环调插件 va_tts_stream_next，节奏宿主可控；
// - 首块即播：第一块 PCM 到达立刻推给播放线程 Sink，首响 = 服务端首块到达；
// - 双设备同源：播放线程同时向默认输出设备（扬声器）与可选虚拟麦克风推流，
//   音量分别取 settings.playback_volume 与 mic_playback_volume；
// - 不中断历史能力：全程 PCM 追加到临时文件，结束后写 WAV 头归档 audio/{id}.wav，
//   消息记录/广播与阻塞路径完全一致（历史可重播、可收藏）；
// - 失败语义：流中途失败即中止播放、不落消息记录（宁缺毋残）；扬声器打开失败时
//   返回 host_played=false，前端回退整段播放（与现状一致）。
//
// 对比埋点：tts_stream_first_audio（首块到达，含连接+握手）/ tts_stream_done（总账）。

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::commands::tts::SynthesizingFlag;
use crate::commands::AppState;
use crate::plugins::{loader::StreamHandle, PluginManager};
use crate::storage::types::{gen_id, now_iso, Message};
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

/// 连接+握手超时（会话级；流中块间隔由插件内 30 秒空闲超时兜底）
const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// 播放线程开局回执等待上限
const PLAYER_ACK_TIMEOUT: Duration = Duration::from_secs(3);

// ── 命令返回 ──────────────────────────────────────────────

/// generate_tts_stream 返回：message 与阻塞路径字段一致；host_played 表示
/// 宿主已承担扬声器流式播放（前端据此跳过整段自动播放，避免双播）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamTtsResult {
    pub message: Message,
    pub host_played: bool,
}

/// 「已开始流式」通知事件名：会话建立、即将出声那一刻就发，让各窗口先把消息
/// 入列（带最终 id 与归档路径），不等整条流跑完——否则用户听得到声音、界面上
/// 却既没有这条消息也没有可点的停止入口。
pub const EVENT_TTS_STREAMING: &str = "tts:streaming";

/// `tts:streaming` 载荷。字段与落库后的 Message 一致（前端拿到正式消息按 id 原地替换），
/// 额外带 engine 供界面区分展示。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamingNotice {
    pub id: String,
    pub content: String,
    pub audio_path: String,
    pub engine: String,
    pub created_at: String,
}

/// 合成失败通知事件名：对已 early 发过 `tts:streaming`（界面已有占位气泡）但中途失败的 id，
/// 补发此事件，前端据此把那条气泡转“失败”态（可重试），不再默默消失。
pub const EVENT_TTS_FAILED: &str = "tts:failed";

/// `tts:failed` 载荷。id 与 `tts:streaming` 一致；reason 为简短中文原因。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TtsFailed {
    pub id: String,
    pub reason: String,
}

/// 流式会话在 tts:streaming 之后失败时广播失败（携带当时那个 id）。
fn announce_stream_failed(app: &AppHandle, id: &str, reason: &str) {
    let _ = app.emit(EVENT_TTS_FAILED, TtsFailed { id: id.to_string(), reason: reason.to_string() });
}

// ── 活跃会话登记（tts_stream_stop 的中止目标）──────────────

struct ActiveStream {
    plugin: Arc<crate::plugins::loader::LoadedPlugin>,
    session: StreamHandle,
}

fn active_stream_slot() -> &'static Mutex<Option<ActiveStream>> {
    static SLOT: OnceLock<Mutex<Option<ActiveStream>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 会话登记守卫：构造时登记活跃会话（供 tts_stream_stop 中止），Drop 时清除
/// （错误/成功路径都经过 Drop，不泄漏）。
struct ActiveGuard {
    _plugin: Arc<crate::plugins::loader::LoadedPlugin>,
    _session: StreamHandle,
}
impl ActiveGuard {
    fn new(plugin: Arc<crate::plugins::loader::LoadedPlugin>, session: StreamHandle) -> Self {
        *active_stream_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(ActiveStream {
            plugin: Arc::clone(&plugin),
            session,
        });
        Self { _plugin: plugin, _session: session }
    }
}
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        *active_stream_slot().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 中止当前流式合成（若在跑）：会话停止 + 立即静音播放。
/// Tauri 命令与宿主能力桥（`native_stop_playback`）共用：宿主的 rodio 流式播放
/// 既不是前端 HTMLAudio、也不是 MicPlayback，两条既有停止链都碰不到它。
pub fn stop_active_stream(app: &AppHandle) {
    let active = active_stream_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(a) = active {
        a.plugin.stream_stop(&a.session);
        // try_state 而非 state：本函数会从插件 C ABI 回调线程被调进，
        // 拿不到管理状态时不能 panic（拿不到也就没什么可中止的）
        if let Some(player) = app.try_state::<StreamPlayer>() {
            player.abort();
        }
        log_info!("流式合成已被用户中止");
    }
}

/// 中止当前流式合成（若在跑）：会话停止 + 立即静音播放
#[tauri::command]
pub fn tts_stream_stop(app: AppHandle) {
    stop_active_stream(&app);
}

/// 静播当前流式播放（若在播）：**只静音**，合成继续跑完、消息与音频文件照常产出。
/// 与 `tts_stream_stop` 的区别就在“不丢产物”；主窗占位气泡上的「⏹ 静播」走这里。
#[tauri::command]
pub fn tts_stream_mute(app: AppHandle) {
    // try_state：本命令可能在会话已收尾后被点（竞态），拿不到就没什么可静的
    if let Some(player) = app.try_state::<StreamPlayer>() {
        player.mute();
    }
}

// ── 主命令 ───────────────────────────────────────────────

#[tauri::command]
pub async fn generate_tts_stream(text: String, app: AppHandle) -> Result<StreamTtsResult, String> {
    generate_tts_stream_impl(&app, &text).await
}

/// generate_tts_stream 核心实现（Tauri 命令与宿主能力桥共用）：
/// 导出流式符号的插件边合边播（`host_played=true`）；
/// 其余引擎内部回落阻塞管线（`host_played=false`，扬声器由前端整段播）。
pub async fn generate_tts_stream_impl(
    app: &AppHandle,
    raw_text: &str,
) -> Result<StreamTtsResult, String> {
    let text = raw_text.trim().to_string();
    if text.is_empty() {
        return Err("文本为空".into());
    }

    // 1. 读设置 + 引擎能力判定
    let (engine_id, plugin_voices, mic_send_enabled, mic_device, mic_volume, speaker_volume) = {
        let state = app.state::<AppState>();
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        (
            s.tts_engine.clone(),
            s.plugin_voices.clone(),
            s.mic_send_enabled,
            s.mic_output_device.clone(),
            s.mic_playback_volume,
            s.playback_volume,
        )
    };
    let plugins = app.state::<PluginManager>();
    let plugin = match plugins.get(&engine_id) {
        Some(p) if p.has_stream() => p,
        // 未导出流式符号的引擎（内置引擎与老插件）：整体回落到既有阻塞管线（宿主不播，前端照旧整段播放）
        _ => {
            let message = crate::commands::tts::generate_tts_impl(&app, &text).await?;
            return Ok(StreamTtsResult { message, host_played: false });
        }
    };

    let _synth_flag = SynthesizingFlag(app.clone());
    crate::plugins::bridge::set_synthesizing(&app, true);

    let content_dir = app.state::<AppState>().content_dir.clone();

    // 2. 打开流式会话（连接+握手；blocking 线程执行）
    let voice = plugin_voices
        .get(&engine_id)
        .filter(|v| !v.is_empty())
        .cloned();
    let t0 = Instant::now();
    let start_task = {
        let plugin = Arc::clone(&plugin);
        let text = text.clone();
        tauri::async_runtime::spawn_blocking(move || {
            plugin.stream_start(&text, voice.as_deref())
        })
    };
    let (session, info) = match tokio::time::timeout(STREAM_CONNECT_TIMEOUT, start_task).await {
        Ok(joined) => joined
            .map_err(|e| format!("流式会话任务中断: {e}"))?
            .map_err(|e| format!("打开流式会话失败: {e}"))?,
        Err(_) => {
            return Err(format!(
                "连接流式合成服务超时（{} 秒），请检查网络与 API Key",
                STREAM_CONNECT_TIMEOUT.as_secs()
            ))
        }
    };
    if info.format != "pcm_s16le" {
        return Err(format!(
            "插件流式格式「{}」不受宿主支持（仅 pcm_s16le）",
            info.format
        ));
    }
    // 建会话耗时单独打点：上面的 t0 在 stream_start 之前，而 tts_stream_first_audio 是从
    // t0 一路算到首块到达（含建会话），所以“开了会话但未开始推流”那一段此前是看不见的。
    // 真机实测：同一个克隆请求，插件直连探针 7~10s，经宿主却 49.5s（两边计时口径不同：
    // 探针从 open() 返回后才起算）。不比这一段就指认不出多出的四十秒在建立会话还是在等服务端。
    // 只记引擎 id，不记文本相关任何内容（日志守卫）；长度已由 tts_stream_first_audio 带。
    log_info!(
        "{}",
        crate::perf::perf_line(
            "stream_open",
            t0.elapsed().as_millis() as u64,
            &[("engine", engine_id.as_str())],
        )
    );
    let channels = info.channels.clamp(1, 2) as u16;
    let sample_rate = if info.sample_rate == 0 { 32000 } else { info.sample_rate };

    // 3. 登记活跃会话 + 播放线程开局（回执设备打开结果）
    let _active_guard = ActiveGuard::new(Arc::clone(&plugin), session);

    // 消息 id 与归档路径先定下来：占位气泡要在出声那一刻就带上最终的 id 与
    // audio_path，命令返回后的正式消息沿用同一对值，前端才能按 id 原地替换
    // （旧实现文件名用 fid、消息另取一个 id，同一段音频会有两个名字）。
    let msg_id = gen_id("m");
    let (wav_rel, tmp_name) = stream_paths_for(&msg_id);
    std::fs::create_dir_all(crate::storage::paths::audio_dir_in(&content_dir))
        .map_err(|e| format!("创建音频目录失败: {e}"))?;
    let tmp_pcm = content_dir.join(&tmp_name);

    let (ack_tx, ack_rx) = channel();
    let has_mic_target = mic_send_enabled && !mic_device.is_empty();
    log_info!(
        "[mic-取证] 流式发送：mic_send_enabled={mic_send_enabled} device=「{mic_device}」 发往麦克风={has_mic_target}"
    );
    let mic_target = if has_mic_target {
        Some((mic_device, mic_volume.clamp(0.0, 1.0)))
    } else {
        None
    };
    app.state::<StreamPlayer>().begin(BeginSpec {
        sample_rate,
        channels,
        speaker_volume: speaker_volume.clamp(0.0, 1.0),
        mic: mic_target,
        ack: ack_tx,
        app: app.clone(),
        wav_rel: wav_rel.clone(),
    });
    let (spk_ok, mic_ok) = match ack_rx.recv_timeout(PLAYER_ACK_TIMEOUT) {
        Ok(v) => v,
        Err(_) => {
            // 开局回执超时不能直接 return：播放线程随后仍会发 va:play:start 并把会话
            // 挂进 active，而拉块任务从未启动（不会有 Chunk/Finish）——那一对 start 永远
            // 等不到 stop（悬浮球始终波纹、遥控看到脏播放态）。Abort 走同一条通道，
            // FIFO 保证它排在 Begin 之后，announce_play_stop 会补发 stop。
            // 真卡死到连 Begin 都没走完时，start 也还没发，状态天然一致。
            cleanup_failed_stream(app, &plugin, &session, &tmp_pcm);
            return Err("流式播放线程无响应".into());
        }
    };
    if !spk_ok {
        log_warn!("流式播放：扬声器打开失败，合成完成后将由前端整段播放");
    } else if !mic_ok && has_mic_target {
        log_warn!("流式播放：虚拟麦克风打开失败，仅扬声器播放");
    }

    // 会话已建立、马上要出声：先把这条“已在合成”的消息告知各窗口（不等整条流跑完）。
    // 扬声器没开成也照发：界面提前可见与谁来播无关。
    let _ = app.emit(
        EVENT_TTS_STREAMING,
        StreamingNotice {
            id: msg_id.clone(),
            content: text.clone(),
            audio_path: wav_rel.clone(),
            engine: engine_id.clone(),
            created_at: now_iso(),
        },
    );

    // 4. 拉块循环（blocking 线程）：块 → 播放 + 追加临时 PCM 文件
    let engine_for_log = engine_id.clone();
    let text_len = text.chars().count();
    let pull_task = {
        let plugin = Arc::clone(&plugin);
        let player_tx = app.state::<StreamPlayer>().sender();
        let tmp = tmp_pcm.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<LoopStats, String> {
            let mut stats = LoopStats::default();
            let mut file = std::io::BufWriter::new(
                std::fs::File::create(&tmp).map_err(|e| format!("创建临时音频失败: {e}"))?,
            );
            loop {
                let chunk = plugin
                    .stream_next(&session)
                    .map_err(|e| e.to_string())?;
                if chunk.is_empty() {
                    break; // 流正常结束
                }
                if stats.first_ms.is_none() {
                    stats.first_ms = Some(t0.elapsed().as_millis() as u64);
                    log_info!(
                        "{}",
                        crate::perf::perf_line(
                            "tts_stream_first_audio",
                            stats.first_ms.unwrap(),
                            &[
                                ("engine", engine_for_log.as_str()),
                                ("len", &text_len.to_string()),
                            ],
                        )
                    );
                }
                stats.chunks += 1;
                stats.pcm_bytes += chunk.len();
                file.write_all(&chunk).map_err(|e| format!("写入临时音频失败: {e}"))?;
                // 16-bit 小端 → i16 采样（插件侧已保证 2 字节对齐；余数安全丢弃）
                let samples: Vec<i16> = chunk
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                if player_tx.send(PlayerCmd::Chunk(samples)).is_err() {
                    return Err("流式播放线程已退出".into());
                }
            }
            file.flush().map_err(|e| format!("写入临时音频失败: {e}"))?;
            let _ = player_tx.send(PlayerCmd::Finish);
            Ok(stats)
        })
    };

    let stats = match pull_task.await {
        Ok(joined) => match joined {
            Ok(stats) => stats,
            Err(e) => {
                announce_stream_failed(&app, &msg_id, &e);
                cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
                return Err(e);
            }
        },
        Err(e) => {
            let reason = format!("流式拉块任务中断: {e}");
            announce_stream_failed(&app, &msg_id, &reason);
            cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
            return Err(reason);
        }
    };
    if stats.chunks == 0 || stats.pcm_bytes == 0 {
        let reason = "流式合成未返回音频".to_string();
        announce_stream_failed(&app, &msg_id, &reason);
        cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
        return Err(reason);
    }

    // 5. PCM → WAV 归档（历史记录可整段重播）
    if let Err(e) = finalize_wav(&tmp_pcm, &content_dir.join(&wav_rel), sample_rate, channels) {
        announce_stream_failed(&app, &msg_id, &e);
        cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
        return Err(e);
    }
    let _ = std::fs::remove_file(&tmp_pcm);

    // 6. 消息记录 + 广播（与阻塞路径一致；扬声器由宿主已流式播过，前端跳过自动播放）
    //    id 与 audio_path 必须与 tts:streaming 事件里那一份一致，前端才能原地替换占位气泡。
    let message = Message {
        id: msg_id,
        content: text.clone(),
        audio_path: wav_rel,
        created_at: now_iso(),
    };
    crate::storage::messages::add_message(&content_dir, message.clone())
        .map_err(|e| format!("保存消息失败: {e}"))?;
    notify_changed(&app, EVENT_MESSAGE_CHANGED);

    log_info!(
        "{}",
        crate::perf::perf_line(
            "tts_stream_done",
            t0.elapsed().as_millis() as u64,
            &[
                ("engine", engine_id.as_str()),
                ("chunks", &stats.chunks.to_string()),
                ("pcm_kb", &(stats.pcm_bytes / 1024).to_string()),
            ],
        )
    );

    Ok(StreamTtsResult {
        message,
        host_played: spk_ok,
    })
}

#[derive(Default)]
struct LoopStats {
    first_ms: Option<u64>,
    chunks: usize,
    pcm_bytes: usize,
}

/// 失败收尾：中止会话与播放、清临时文件（消息不落库，宁缺毋残）
fn cleanup_failed_stream(
    app: &AppHandle,
    plugin: &Arc<crate::plugins::loader::LoadedPlugin>,
    session: &StreamHandle,
    tmp_pcm: &Path,
) {
    plugin.stream_stop(session);
    app.state::<StreamPlayer>().abort();
    let _ = std::fs::remove_file(tmp_pcm);
}

// ── WAV 归档 ─────────────────────────────────────────────

/// 44 字节标准 PCM WAV 头 + 拷贝临时 PCM；data_len 事后按实际长度定稿。
fn finalize_wav(tmp_pcm: &Path, wav_out: &Path, sample_rate: u32, channels: u16) -> Result<(), String> {
    let pcm_len = std::fs::metadata(tmp_pcm)
        .map_err(|e| format!("读取临时音频失败: {e}"))?
        .len();
    if pcm_len == 0 || pcm_len > u32::MAX as u64 {
        return Err("流式合成音频为空或超出 WAV 容量上限".into());
    }
    let bits = 16u16;
    let byte_rate = sample_rate * channels as u32 * (bits / 8) as u32;
    let block_align = channels * (bits / 8);
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(wav_out).map_err(|e| format!("创建音频文件失败: {e}"))?,
    );
    out.write_all(b"RIFF").map_err(wav_io)?;
    out.write_all(&(36 + pcm_len as u32).to_le_bytes()).map_err(wav_io)?;
    out.write_all(b"WAVE").map_err(wav_io)?;
    out.write_all(b"fmt ").map_err(wav_io)?;
    out.write_all(&16u32.to_le_bytes()).map_err(wav_io)?; // fmt 块长度
    out.write_all(&1u16.to_le_bytes()).map_err(wav_io)?; // PCM
    out.write_all(&channels.to_le_bytes()).map_err(wav_io)?;
    out.write_all(&sample_rate.to_le_bytes()).map_err(wav_io)?;
    out.write_all(&byte_rate.to_le_bytes()).map_err(wav_io)?;
    out.write_all(&block_align.to_le_bytes()).map_err(wav_io)?;
    out.write_all(&bits.to_le_bytes()).map_err(wav_io)?;
    out.write_all(b"data").map_err(wav_io)?;
    out.write_all(&(pcm_len as u32).to_le_bytes()).map_err(wav_io)?;
    let mut src = std::fs::File::open(tmp_pcm).map_err(|e| format!("读取临时音频失败: {e}"))?;
    std::io::copy(&mut src, &mut out).map_err(wav_io)?;
    out.flush().map_err(wav_io)?;
    Ok(())
}

fn wav_io(e: std::io::Error) -> String {
    format!("写入 WAV 失败: {e}")
}

// ── 播放线程（rodio OutputStream 非 Send，参考 mic.rs 关进专用线程）──

/// 开局参数（generate_tts_stream_impl → 播放线程）
pub struct BeginSpec {
    pub sample_rate: u32,
    pub channels: u16,
    /// 扬声器音量（settings.playback_volume）
    pub speaker_volume: f32,
    /// Some((设备名, 音量))：虚拟麦克风同源推流
    pub mic: Option<(String, f32)>,
    /// 开局回执：(扬声器已打开, 虚拟麦克风已打开)
    pub ack: Sender<(bool, bool)>,
    /// 应用句柄：`rodio::OutputStream` 不是 Send，但 `AppHandle` 是，
    /// 播放态事件就在这个线程里发
    pub app: AppHandle,
    /// 本次流式归档的音频相对路径（`playback:started` 的载荷，能力桥据此聚合播放态）
    pub wav_rel: String,
}

enum PlayerCmd {
    Begin(BeginSpec),
    Chunk(Vec<i16>),
    /// 合成结束：待 Sink 排空后自然收尾
    Finish,
    /// 立即静音并丢弃当前流
    Abort,
    /// 静播：只停“响”（丢缓冲 + 释放设备），**不中止会话**——拉块与写文件继续，
    /// 消息与音频照常完成。与 Abort 的区分就是用户要的“东西还是得产出来”。
    Mute,
}

/// 流式播放全局句柄（只含 Sender，天生 Send+Sync，可 manage）
pub struct StreamPlayer {
    tx: Sender<PlayerCmd>,
}

impl StreamPlayer {
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<PlayerCmd>();
        std::thread::Builder::new()
            .name("voiceassist-tts-stream".into())
            .spawn(move || player_thread(rx))
            .expect("启动流式播放线程失败");
        StreamPlayer { tx }
    }

    fn sender(&self) -> Sender<PlayerCmd> {
        self.tx.clone()
    }

    fn begin(&self, spec: BeginSpec) {
        let _ = self.tx.send(PlayerCmd::Begin(spec));
    }

    fn abort(&self) {
        let _ = self.tx.send(PlayerCmd::Abort);
    }

    fn mute(&self) {
        let _ = self.tx.send(PlayerCmd::Mute);
    }
}

/// 一个设备的播放通路：OutputStream 必须与 Sink 同存保活
struct DeviceSink {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
}

struct PlaySession {
    sample_rate: u32,
    channels: u16,
    sinks: Vec<DeviceSink>,
    finishing: bool,
    /// 已被用户静播：后续块不再喂设备（但拉块与写文件照常进行）
    muted: bool,
    app: AppHandle,
    /// 开局时宿主是否发过 `va:play:start`（决定收尾要不要发对应的那一套 stop）
    announced: bool,
    /// 本次宿主流式播放的归档相对路径（供 `va:hostplay:*` 前端同步气泡播放态）
    wav_rel: String,
}

/// 宿主播完/中断时的播放态收尾：只补发开局时发过 start 的那一套。
/// 前端整段播放路径自己会发一套，两边不发两套是 `announced` 标志存在的全部意义。
/// `va:hostplay:stop` 是宿主专用频道（前端只消费不发送），带上 wav_rel 供气泡精确清除高亮。
fn announce_play_stop(s: &PlaySession) {
    if !s.announced {
        return;
    }
    let _ = s.app.emit("va:play:stop", ());
    let _ = s.app.emit("playback:stopped", ());
    let _ = s.app.emit("va:hostplay:stop", s.wav_rel.clone());
}

fn player_thread(rx: std::sync::mpsc::Receiver<PlayerCmd>) {
    let mut active: Option<PlaySession> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PlayerCmd::Begin(spec)) => {
                // 新流替换旧流（未播完的尾巴直接切断，与虚拟麦克风 Play 语义一致）
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
                let mut sinks: Vec<DeviceSink> = Vec::new();
                let mut spk_ok = false;
                match rodio::OutputStream::try_default() {
                    Ok((stream, handle)) => match rodio::Sink::try_new(&handle) {
                        Ok(sink) => {
                            sink.set_volume(spec.speaker_volume);
                            sinks.push(DeviceSink { _stream: stream, sink });
                            spk_ok = true;
                        }
                        Err(e) => log_warn!("流式播放：创建扬声器 Sink 失败: {e}"),
                    },
                    Err(e) => log_warn!("流式播放：打开默认输出设备失败: {e}"),
                }
                let mut mic_ok = false;
                if let Some((device, volume)) = &spec.mic {
                    match crate::commands::mic::find_device_by_name(device)
                        .ok_or_else(|| format!("未找到音频设备：{device}"))
                        .and_then(|d| {
                            rodio::OutputStream::try_from_device(&d)
                                .map_err(|e| crate::commands::mic::describe_device_open_failure(device, &e.to_string()))
                        }) {
                        Ok((stream, handle)) => match rodio::Sink::try_new(&handle) {
                            Ok(sink) => {
                                sink.set_volume(*volume);
                                sinks.push(DeviceSink { _stream: stream, sink });
                                mic_ok = true;
                                log_info!("[mic-取证] 流式已开播到虚拟麦克风");
                            }
                            Err(e) => log_warn!("流式播放：创建虚拟麦克风 Sink 失败: {e}"),
                        },
                        Err(e) => log_warn!("流式播放：{e}"),
                    }
                }
                let _ = spec.ack.send((spk_ok, mic_ok));
                let app = spec.app.clone();
                let wav_rel = spec.wav_rel.clone();
                // 只在扬声器真开成时发：开不成时 host_played=false，
                // 前端会整段播放并自行发一套事件（两边都发会双计）
                let announced = spk_ok;
                if announced {
                    // 注意：此刻 WAV 尚未落盘（步骤 5 才写），载荷只是「本次将归档到哪个路径」的
                    // 标识；现有消费方（bridge 聚合播放态、state_json 反查收藏 id）不取文件，
                    // 后续若要做「边播边取/进度显示」需改成播完再发或带时长
                    let _ = app.emit("va:play:start", ());
                    let _ = app.emit("playback:started", wav_rel.clone());
                    // 宿主专用：前端据此把那条气泡标为「播放中」，修 bug④（点它=停不双播）
                    let _ = app.emit("va:hostplay:start", wav_rel.clone());
                }
                if !sinks.is_empty() {
                    active = Some(PlaySession {
                        sample_rate: spec.sample_rate,
                        channels: spec.channels,
                        sinks,
                        finishing: false,
                        muted: false,
                        app,
                        announced,
                        wav_rel,
                    });
                }
            }
            Ok(PlayerCmd::Chunk(samples)) => {
                if let Some(s) = &mut active {
                    // 静播或已无设备时直接丢块（拉块循环在另一线程，文件仍会写完整）
                    if should_feed(s.muted, !s.sinks.is_empty()) {
                        for ds in &mut s.sinks {
                            ds.sink.append(rodio::buffer::SamplesBuffer::new(
                                s.channels,
                                s.sample_rate,
                                samples.clone(),
                            ));
                        }
                    }
                }
            }
            Ok(PlayerCmd::Finish) => {
                if let Some(s) = &mut active {
                    s.finishing = true;
                }
            }
            Ok(PlayerCmd::Abort) => {
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
            }
            Ok(PlayerCmd::Mute) => {
                if let Some(s) = active.as_mut() {
                    // 静播只停“响”：丢缓冲并释放设备（不 Abort 会话、不停拉块）。
                    let (muted, need_stop_notice) = mute_transitions(s.announced);
                    s.muted = muted;
                    s.sinks.clear();
                    if need_stop_notice {
                        // 补发一次停止，并抹掉 announced：收尾时不会再发第二套
                        announce_play_stop(s);
                        s.announced = false;
                    }
                    log_info!("流式播放：已静播（合成继续，消息与音频照常完成）");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let drained = active
                    .as_mut()
                    .map(|s| s.finishing && s.sinks.iter().all(|ds| ds.sink.empty()))
                    .unwrap_or(false);
                if drained {
                    // 排空完成：先报停止再释放设备，订阅方不会看到「播完还在播」
                    if let Some(prev) = active.take() {
                        announce_play_stop(&prev);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // 兜底路径（正常生命周期不可达：StreamPlayer 被 manage，其 tx 与进程同寿，
                // 拉块任务只 drop 自己的副本）：真到了这一步就补发停止事件后退出
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
                break;
            }
        }
    }
}

// ── 可单测的小决策（纯函数）───────────────────────

/// 由消息 id 推导本次流式的归档与临时文件路径。
/// 单一来源：“出声前就发给界面的占位消息”与“播完落库的正式消息”必须指向同一个
/// audio_path，否则前端无法原地替换（会变成两条一模一样的消息）。
fn stream_paths_for(msg_id: &str) -> (String, String) {
    use crate::storage::paths::AUDIO_DIR_NAME;
    (
        format!("{AUDIO_DIR_NAME}/{msg_id}.wav"),
        format!("{AUDIO_DIR_NAME}/{msg_id}.pcm.tmp"),
    )
}

/// 这一块要不要喂给播放设备：静播后、或设备根本没开成时丢弃。
/// 注意丢的只是喂设备——写临时文件在拉块线程里，不受本函数影响。
fn should_feed(muted: bool, has_sinks: bool) -> bool {
    !muted && has_sinks
}

/// 静播时的状态转移：返回（新的 muted 值, 是否需要补发一次停止事件）。
/// 只有原本真的报过“正在播”（扬声器开成且 announced）才补发；
/// 否则会出现悬浮球/遥控收到两套 va:play:stop。
fn mute_transitions(announced: bool) -> (bool, bool) {
    (true, announced)
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(pcm_len: u32, rate: u32, channels: u16) -> Vec<u8> {
        // 用 finalize_wav 的字节序列生成器直接构造，校验头部各字段
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("a.pcm");
        std::fs::write(&tmp, vec![0u8; pcm_len as usize]).unwrap();
        let out = dir.path().join("a.wav");
        finalize_wav(&tmp, &out, rate, channels).unwrap();
        std::fs::read(&out).unwrap()
    }

    #[test]
    fn wav_头部规范字段() {
        let bytes = wav_bytes(4000, 32000, 1);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 32000);
        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), 64000); // byte_rate
        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 2); // block_align
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16); // bits
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 4000);
        assert_eq!(bytes.len(), 44 + 4000);
    }

    #[test]
    fn wav_立体声字段() {
        let bytes = wav_bytes(1000, 44100, 2);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), 176400);
        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 4);
    }

    #[test]
    fn wav_空pcm报错() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("a.pcm");
        std::fs::write(&tmp, b"").unwrap();
        assert!(finalize_wav(&tmp, &dir.path().join("a.wav"), 32000, 1).is_err());
    }

    #[test]
    fn pcm_小端转采样() {
        let bytes = [0x01u8, 0x80, 0xFF, 0x7F]; // LE: 0x8001, 0x7FFF
        let samples: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        assert_eq!(samples, vec![-32767, 32767]);
    }

    // ── Task 15：即发即入框 + 可静播 ──────────────────────

    #[test]
    fn 路径预分配_消息id与归档名一致() {
        // 占位气泡要能在命令返回前就带出最终的 audio_path，而正式消息必须沿用
        // 同一个 id 与路径——故两者都从消息 id 推导，不能再各自 gen_id
        // （旧实现里文件名用 fid、消息用另一个 id，同一条音频两个名字）。
        let (wav_rel, tmp_name) = stream_paths_for("m_42");
        assert_eq!(wav_rel, "audio/m_42.wav");
        assert_eq!(tmp_name, "audio/m_42.pcm.tmp");
    }

    #[test]
    fn 静播时丢块但仍继续拉流与写文件() {
        // 静播的语义是「不响但东西要产出来」：所以丢的是喂给设备的动作，
        // 拉块循环与临时文件写入完全不受影响。
        assert!(!should_feed(true, true), "静播后不再喂设备");
        assert!(!should_feed(false, false), "设备没开成时无东西可喂");
        assert!(should_feed(false, true), "正常流式应喂设备");
    }

    #[test]
    fn 静播只在原本报过正在播时补发停止() {
        // announced=false 的两种情形（扬声器没开成 / 已经静过一次）都不能再发第二套
        // va:play:stop，否则悬浮球与遥控会跳两下。
        assert_eq!(mute_transitions(true), (true, true));
        assert_eq!(mute_transitions(false), (true, false));
    }
}
