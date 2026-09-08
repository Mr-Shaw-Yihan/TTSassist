// WASAPI loopback 采集：抓默认渲染设备（扬声器）的系统混音，下混单声道并重采样到
// 16kHz f32，通过通道源源不断吐给会话编排线程。
//
// 为什么用系统级 loopback 而非进程级：
// - 进程级隔离要 ActivateAudioInterfaceAsync + 音频处理对象（APO），复杂且难盲测；
// - 系统级捕获整条混音，靠 VAD 过滤非语音即可满足「听到队友/游戏语音才转写」，
//   实现可控。self-TTS 回采（软件自己播放被再次抓到）用 playback 事件暂停会话规避。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    eConsole, eRender, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient,
    IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

use super::{ComGuard, TARGET_SAMPLE_RATE};

/// WAVEFORMATEX.wFormatTag 常见值。
const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

/// 重采样器：线性插值，跨调用保持相位连续，输出缓冲有界。
///
/// 输入是单声道 f32（设备采样率），输出单声道 f32（目标采样率）。
struct LinearResampler {
    /// 每个输出样本要消耗多少输入样本 = in_rate / out_rate
    step: f64,
    /// 未消费的输入样本
    buf: Vec<f32>,
    /// buf[0] 在输入流中的绝对索引
    buf_start: i64,
    /// 下一个输出样本对应的绝对浮点读位置
    cursor: f64,
}

impl LinearResampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            step: in_rate as f64 / out_rate as f64,
            buf: Vec::new(),
            buf_start: 0,
            cursor: 0.0,
        }
    }

    fn push(&mut self, chunk: &[f32], out: &mut Vec<f32>) {
        if chunk.is_empty() {
            return;
        }
        self.buf.extend_from_slice(chunk);
        let n = self.buf.len() as i64;
        loop {
            let floor = self.cursor.floor();
            let idx = floor as i64 - self.buf_start;
            // 需要 buf[idx] 与 buf[idx+1]
            if idx < 0 || idx + 1 >= n {
                break;
            }
            let frac = (self.cursor - floor) as f32;
            let s0 = self.buf[idx as usize];
            let s1 = self.buf[idx as usize + 1];
            out.push(s0 + (s1 - s0) * frac);
            self.cursor += self.step;
        }
        // 裁剪已消费的输入（保留 cursor 前一个样本用于下次插值）
        let keep_from = (self.cursor.floor() as i64 - 1).max(self.buf_start);
        let drop = (keep_from - self.buf_start) as usize;
        if drop > 0 {
            self.buf.drain(..drop);
            self.buf_start += drop as i64;
        }
    }
}

/// 采集线程返回句柄三元组：`(停止标志, 16k 单声道 f32 样本接收端, 线程 join)`。
pub type CaptureHandles = (Arc<AtomicBool>, Receiver<Vec<f32>>, JoinHandle<()>);

/// 启动一路 loopback 采集后台线程。
///
/// 返回 `(stop, rx, join)`：
/// - 向 `stop` 写 true 请求线程退出；
/// - `rx` 收到 16kHz 单声道 f32 样本块；
/// - `join` 用于等待线程真正结束（确保释放 COM/WASAPI 资源）。
///
/// 失败（无默认设备、COM 初始化失败等）返回 Err。
pub fn spawn_capture() -> Result<CaptureHandles, String> {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();

    let join = std::thread::Builder::new()
        .name("asr-loopback-capture".into())
        .spawn(move || run_capture(stop2, tx))
        .map_err(|e| format!("启动采集线程失败：{e}"))?;

    Ok((stop, rx, join))
}

fn run_capture(stop: Arc<AtomicBool>, tx: mpsc::Sender<Vec<f32>>) {
    let _com = ComGuard::new_mta();
    if let Err(e) = capture_inner(&stop, &tx) {
        eprintln!("[asr-subtitle] 采集线程异常退出：{e}");
    }
}

fn capture_inner(stop: &AtomicBool, tx: &mpsc::Sender<Vec<f32>>) -> Result<(), String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("创建设备枚举器失败：{e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("无默认播放设备：{e}"))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("激活 IAudioClient 失败：{e}"))?;

        // 设备混音格式（共享模式 loopback 必须用它）
        let mix_fmt = client.GetMixFormat().map_err(|e| format!("获取混音格式失败：{e}"))?;
        if mix_fmt.is_null() {
            return Err("混音格式为空".into());
        }
        let fmt = *mix_fmt;
        let channels = (fmt.nChannels.max(1)) as usize;
        let device_rate = fmt.nSamplesPerSec.max(1);
        let bits = fmt.wBitsPerSample;
        let tag = fmt.wFormatTag;

        // 共享 loopback：buffer 时长传 0（由系统决定），周期传 0
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                0,
                0,
                mix_fmt as *const _,
                None,
            )
            .map_err(|e| format!("初始化 loopback 失败：{e}"))?;

        CoTaskMemFree(Some(mix_fmt as *const _));

        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| format!("获取 IAudioCaptureClient 失败：{e}"))?;

        client.Start().map_err(|e| format!("启动采集失败：{e}"))?;

        let mut resampler = LinearResampler::new(device_rate, TARGET_SAMPLE_RATE);
        let is_float = tag == WAVE_FORMAT_IEEE_FLOAT;
        let is_pcm16 = tag == WAVE_FORMAT_PCM && bits == 16;
        if !is_float && !is_pcm16 {
            client.Stop().ok();
            return Err(format!("不支持的混音格式 tag={tag} bits={bits}"));
        }

        let mut mono: Vec<f32> = Vec::new();
        let mut out: Vec<f32> = Vec::new();

        while !stop.load(Ordering::Relaxed) {
            let mut packets = capture.GetNextPacketSize().map_err(|e| format!("{e}"))?;
            while packets != 0 {
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut nframes: u32 = 0;
                let mut flags: u32 = 0;
                capture
                    .GetBuffer(&mut data, &mut nframes, &mut flags, None, None)
                    .map_err(|e| format!("{e}"))?;

                mono.clear();
                let frames = nframes as usize;
                if is_float {
                    let p = data as *const f32;
                    let total = frames * channels;
                    let slice = std::slice::from_raw_parts(p, total);
                    downmix(slice, channels, &mut mono);
                } else {
                    let p = data as *const i16;
                    let total = frames * channels;
                    let slice = std::slice::from_raw_parts(p, total);
                    downmix_i16(slice, channels, &mut mono);
                }

                capture.ReleaseBuffer(nframes).map_err(|e| format!("{e}"))?;

                out.clear();
                resampler.push(&mono, &mut out);
                if !out.is_empty() && tx.send(std::mem::take(&mut out)).is_err() {
                    // 接收端已关闭 → 停止
                    client.Stop().ok();
                    return Ok(());
                }

                packets = capture.GetNextPacketSize().map_err(|e| format!("{e}"))?;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        client.Stop().ok();
        Ok(())
    }
}

/// 多声道 f32 → 单声道（取平均）。
fn downmix(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let frames = interleaved.len() / channels;
    out.reserve(frames);
    for f in 0..frames {
        let base = f * channels;
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += interleaved[base + c];
        }
        out.push(sum / channels as f32);
    }
}

/// 多声道 i16 → 单声道 f32（归一化到 [-1,1]，取平均）。
fn downmix_i16(interleaved: &[i16], channels: usize, out: &mut Vec<f32>) {
    let scale = 1.0 / 32768.0;
    let frames = interleaved.len() / channels.max(1);
    out.reserve(frames);
    for f in 0..frames {
        let base = f * channels;
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += interleaved[base + c] as f32;
        }
        out.push(sum / channels as f32 * scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slope(n: usize) -> Vec<f32> {
        (0..n).map(|i| i as f32).collect()
    }

    #[test]
    fn identity_when_rates_equal() {
        let mut r = LinearResampler::new(16000, 16000);
        let mut out = Vec::new();
        r.push(&slope(100), &mut out);
        // step=1，线性插值在整数网格上取到原值（首个输出=buf[0]=0）
        assert!(out.len() >= 98, "输出长度应≈输入，实际 {}", out.len());
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 1.0);
    }

    #[test]
    fn downsamples_by_ratio_length() {
        // 48k→16k：step=3，输出样本数≈输入/3
        let mut r = LinearResampler::new(48000, 16000);
        let mut out = Vec::new();
        r.push(&slope(3000), &mut out);
        let expected = 3000.0 / 3.0;
        let diff = (out.len() as f64 - expected).abs();
        assert!(diff <= 3.0, "48k→16k 输出应≈1000，实际 {}", out.len());
    }

    #[test]
    fn upsamples_and_preserves_linearity() {
        // 8k→16k：step=0.5，线性斜坡插值应保持值
        let mut r = LinearResampler::new(8000, 16000);
        let mut out = Vec::new();
        r.push(&slope(100), &mut out);
        assert!(out.len() >= 195, "8k→16k 输出应≈200，实际 {}", out.len());
        assert_eq!(out[0], 0.0);
        assert_eq!(out[2], 1.0); // cursor 每步 +0.5 → out[2] 落在输入样本 1
    }

    #[test]
    fn cross_chunk_continuity() {
        // 分两块喂，应等价于一次喂入（输出单调递增，无回跳）
        let mut r = LinearResampler::new(48000, 16000);
        let mut out = Vec::new();
        r.push(&slope(1000), &mut out);
        let first_len = out.len();
        r.push(&slope(1000).into_iter().map(|v| v + 1000.0).collect::<Vec<f32>>(), &mut out);
        assert!(out.len() > first_len);
        // 无 NaN、单调不减（斜坡）
        assert!(out.windows(2).all(|w| w[0] <= w[1] + 1e-3), "输出应保持单调");
    }

    #[test]
    fn downmix_averages_channels() {
        let inter = vec![0.0, 1.0, 0.5, 0.5]; // 2 声道 × 2 帧
        let mut out = Vec::new();
        downmix(&inter, 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn downmix_i16_normalizes() {
        let inter = vec![0i16, 32767i16, -32768i16, 0i16];
        let mut out = Vec::new();
        downmix_i16(&inter, 2, &mut out);
        assert!((out[0] - 32767.0 / 2.0 / 32768.0).abs() < 1e-3);
        assert!((out[1] - (-32768.0 / 2.0 / 32768.0)).abs() < 1e-3);
    }
}
