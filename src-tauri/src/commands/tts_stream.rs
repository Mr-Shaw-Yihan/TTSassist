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

use tauri::{AppHandle, Manager};

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

/// 中止当前流式合成（若在跑）：会话停止 + 立即静音播放
#[tauri::command]
pub fn tts_stream_stop(app: AppHandle) {
    let active = active_stream_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(a) = active {
        a.plugin.stream_stop(&a.session);
        app.state::<StreamPlayer>().abort();
        log_info!("流式合成已被用户中止");
    }
}

// ── 主命令 ───────────────────────────────────────────────

#[tauri::command]
pub async fn generate_tts_stream(text: String, app: AppHandle) -> Result<StreamTtsResult, String> {
    let text = text.trim().to_string();
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
        // 非流式引擎（mimo/moss/edge-tts 等）：整体回落到既有阻塞管线（宿主不播，前端照旧整段播放）
        _ => {
            let message = crate::commands::tts::generate_tts_impl(&app, &text).await?;
            return Ok(StreamTtsResult { message, host_played: false });
        }
    };

    let _synth_flag = SynthesizingFlag(app.clone());
    crate::plugins::bridge::set_synthesizing(&app, true);

    let data_dir = app.state::<AppState>().data_dir.clone();

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
    let channels = info.channels.clamp(1, 2) as u16;
    let sample_rate = if info.sample_rate == 0 { 32000 } else { info.sample_rate };

    // 3. 登记活跃会话 + 播放线程开局（回执设备打开结果）
    let _active_guard = ActiveGuard::new(Arc::clone(&plugin), session);
    let (ack_tx, ack_rx) = channel();
    let has_mic_target = mic_send_enabled && !mic_device.is_empty();
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
    });
    let (spk_ok, mic_ok) = ack_rx
        .recv_timeout(PLAYER_ACK_TIMEOUT)
        .map_err(|_| "流式播放线程无响应".to_string())?;
    if !spk_ok {
        log_warn!("流式播放：扬声器打开失败，合成完成后将由前端整段播放");
    } else if !mic_ok && has_mic_target {
        log_warn!("流式播放：虚拟麦克风打开失败，仅扬声器播放");
    }

    // 4. 拉块循环（blocking 线程）：块 → 播放 + 追加临时 PCM 文件
    let fid = gen_id("m");
    std::fs::create_dir_all(data_dir.join("audio"))
        .map_err(|e| format!("创建音频目录失败: {e}"))?;
    let tmp_pcm = data_dir.join(format!("audio/{fid}.pcm.tmp"));
    let wav_rel = format!("audio/{fid}.wav");
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
                cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
                return Err(e);
            }
        },
        Err(e) => {
            cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
            return Err(format!("流式拉块任务中断: {e}"));
        }
    };
    if stats.chunks == 0 || stats.pcm_bytes == 0 {
        cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
        return Err("流式合成未返回音频".into());
    }

    // 5. PCM → WAV 归档（历史记录可整段重播）
    if let Err(e) = finalize_wav(&tmp_pcm, &data_dir.join(&wav_rel), sample_rate, channels) {
        cleanup_failed_stream(&app, &plugin, &session, &tmp_pcm);
        return Err(e);
    }
    let _ = std::fs::remove_file(&tmp_pcm);

    // 6. 消息记录 + 广播（与阻塞路径一致；扬声器由宿主已流式播过，前端跳过自动播放）
    let message = Message {
        id: gen_id("m"),
        content: text.clone(),
        audio_path: wav_rel,
        created_at: now_iso(),
    };
    crate::storage::messages::add_message(&data_dir, message.clone())
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

/// 开局参数（generate_tts_stream → 播放线程）
pub struct BeginSpec {
    pub sample_rate: u32,
    pub channels: u16,
    /// 扬声器音量（settings.playback_volume）
    pub speaker_volume: f32,
    /// Some((设备名, 音量))：虚拟麦克风同源推流
    pub mic: Option<(String, f32)>,
    /// 开局回执：(扬声器已打开, 虚拟麦克风已打开)
    pub ack: Sender<(bool, bool)>,
}

enum PlayerCmd {
    Begin(BeginSpec),
    Chunk(Vec<i16>),
    /// 合成结束：待 Sink 排空后自然收尾
    Finish,
    /// 立即静音并丢弃当前流
    Abort,
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
}

fn player_thread(rx: std::sync::mpsc::Receiver<PlayerCmd>) {
    let mut active: Option<PlaySession> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PlayerCmd::Begin(spec)) => {
                // 新流替换旧流（未播完的尾巴直接切断，与虚拟麦克风 Play 语义一致）
                active = None;
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
                                .map_err(|e| format!("无法打开设备 {device}：{e}"))
                        }) {
                        Ok((stream, handle)) => match rodio::Sink::try_new(&handle) {
                            Ok(sink) => {
                                sink.set_volume(*volume);
                                sinks.push(DeviceSink { _stream: stream, sink });
                                mic_ok = true;
                            }
                            Err(e) => log_warn!("流式播放：创建虚拟麦克风 Sink 失败: {e}"),
                        },
                        Err(e) => log_warn!("流式播放：{e}"),
                    }
                }
                let _ = spec.ack.send((spk_ok, mic_ok));
                if !sinks.is_empty() {
                    active = Some(PlaySession {
                        sample_rate: spec.sample_rate,
                        channels: spec.channels,
                        sinks,
                        finishing: false,
                    });
                }
            }
            Ok(PlayerCmd::Chunk(samples)) => {
                if let Some(s) = &mut active {
                    for ds in &mut s.sinks {
                        ds.sink.append(rodio::buffer::SamplesBuffer::new(
                            s.channels,
                            s.sample_rate,
                            samples.clone(),
                        ));
                    }
                }
            }
            Ok(PlayerCmd::Finish) => {
                if let Some(s) = &mut active {
                    s.finishing = true;
                }
            }
            Ok(PlayerCmd::Abort) => active = None,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(s) = &mut active {
                    if s.finishing && s.sinks.iter().all(|ds| ds.sink.empty()) {
                        active = None; // 排空完成，收尾释放设备
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
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
}
