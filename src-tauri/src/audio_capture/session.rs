// 监听会话编排：采集线程 → 能量 VAD 切句 → WAV 编码 → ASR 插件转写 → emit 字幕。
//
// 线程模型：
// - 本函数所在的「会话线程」从采集通道取 16k mono 样本喂 VAD；
// - 每切出一句就编码成 WAV，丢进 tauri 的 blocking 池做 FFI 转写（转写耗时不阻塞采集）；
// - 转写成功且非空 → 追加到当前会话历史 + emit `asr:subtitle` 给浮窗。
//
// 暂停（Alt+M）：paused 标志为 true 时，会话线程直接丢弃样本、不喂 VAD 不转写（省钱）。

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use super::capture;
use super::vad::EnergyVad;
use super::{SessionConfig, SubtitleLine, ASR_SUBTITLE_EVENT, TARGET_SAMPLE_RATE};
use crate::plugins::LoadedAsrPlugin;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把单声道 f32（[-1,1]）编码为 16bit/mono/`rate` 的 WAV 字节（ASR 插件约定格式）。
fn encode_wav16(samples: &[f32], rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        // WavWriter::new 只在内存 Cursor 上写，不会失败到需要 panic 的程度；
        // 但为稳起见出错退化为空缓冲。
        if let Ok(mut writer) = hound::WavWriter::new(&mut cursor, spec) {
            for &s in samples {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                let _ = writer.write_sample(v);
            }
            let _ = writer.finalize();
        }
    }
    cursor.into_inner()
}

/// 处理一句语音：编码 → blocking 转写 → 非空则入历史 + emit。
fn handle_segment(
    app: &AppHandle,
    plugin: &Arc<LoadedAsrPlugin>,
    cfg: &SessionConfig,
    samples: Vec<f32>,
    current: &Arc<Mutex<Vec<SubtitleLine>>>,
) {
    let wav = encode_wav16(&samples, TARGET_SAMPLE_RATE);
    let lang = cfg.asr_language().map(|s| s.to_string());
    let plugin_id = cfg.plugin_id.clone();
    let app = app.clone();
    let plugin = plugin.clone();
    let current = current.clone();

    tauri::async_runtime::spawn_blocking(move || {
        match plugin.transcribe(&wav, lang.as_deref()) {
            Ok(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return; // ASR 判定无有效语音 → 丢弃
                }
                let line = SubtitleLine {
                    text: text.to_string(),
                    ts: now_ms(),
                };
                if let Ok(mut g) = current.lock() {
                    g.push(line.clone());
                }
                let _ = app.emit(ASR_SUBTITLE_EVENT, &line);
            }
            Err(e) => {
                eprintln!("[asr-subtitle] 转写失败（插件 {plugin_id}）：{e}");
            }
        }
    });
}

/// 启动一路监听会话，返回其会话线程句柄。
///
/// 调用方持有：
/// - `stop`：置 true 请求会话线程收尾退出（会 join 采集线程）；
/// - `current`：会话结束后从里面取走本轮字幕（落盘为一条 session）。
pub fn spawn_session(
    app: AppHandle,
    plugin: Arc<LoadedAsrPlugin>,
    cfg: SessionConfig,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Vec<SubtitleLine>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let (cap_stop, rx, cap_join) = match capture::spawn_capture() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[asr-subtitle] 采集启动失败：{e}");
                return;
            }
        };

        let mut vad = EnergyVad::new(cfg.vad.clone(), TARGET_SAMPLE_RATE);
        let mut leftover: Vec<f32> = Vec::new();

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    // 暂停：丢弃样本，不喂 VAD、不转写
                    if paused.load(Ordering::Relaxed) {
                        continue;
                    }
                    vad.push(&chunk, &mut leftover, &mut |seg| {
                        handle_segment(&app, &plugin, &cfg, seg.samples, &current)
                    });
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        // 收尾：把仍在缓冲、尚未触发 hangover 的末句强制冲出
        vad.finish(&mut |seg| handle_segment(&app, &plugin, &cfg, seg.samples, &current));

        // 关闭采集线程并等待其释放 WASAPI/COM 资源
        cap_stop.store(true, Ordering::Relaxed);
        let _ = cap_join.join();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_valid_16bit_mono() {
        let samples = vec![0.0f32; 1600];
        let wav = encode_wav16(&samples, TARGET_SAMPLE_RATE);
        // RIFF....WAVE
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 用 hound 读回校验规格
        let reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        assert_eq!(reader.duration(), 1600);
    }

    #[test]
    fn wav_encodes_peak_and_clamps() {
        let samples = vec![1.0, -1.0, 2.0, -2.0]; // 后两个越界应被 clamp
        let wav = encode_wav16(&samples, TARGET_SAMPLE_RATE);
        let mut reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        let vals: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(vals[0], 32767);
        assert_eq!(vals[1], -32767);
        assert_eq!(vals[2], 32767); // clamp(2.0)=1.0
        assert_eq!(vals[3], -32767); // clamp(-2.0)=-1.0
    }
}
