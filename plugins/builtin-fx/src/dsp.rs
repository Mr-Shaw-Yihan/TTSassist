// 纯 DSP 数学：全部函数确定性、无 IO、无状态逃逸，可脱离插件单独测试（设计 §十一）。
//
// 铁律：效果输出必须限幅 [-1, 1] 且峰值归一（防响度暴涨刺耳）；静音输入必须仍为静音。

use serde_json::Value;

/// 从参数 JSON（{"k":"v"} 或 {"k":v}，字符串/数字兼容）取 f64 参数
pub fn param_f64(params_json: &str, key: &str, default: f64) -> f64 {
    let Ok(v) = serde_json::from_str::<Value>(params_json) else {
        return default;
    };
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(default),
        _ => default,
    }
}

/// 软削波：tanh waveshaping。drive=1 时近似线性（小幅），越大越「糊」
#[inline]
pub fn soft_clip(x: f32, drive: f32) -> f32 {
    (x * drive).tanh()
}

/// 峰值归一到 target_peak（≤1.0）；全静音保持静音
pub fn normalize(pcm: &mut [f32], target_peak: f32) {
    let peak = pcm.iter().fold(0f32, |m, s| m.max(s.abs()));
    if peak > 1e-9 {
        let g = target_peak / peak;
        if (g - 1.0).abs() > 1e-9 {
            for s in pcm.iter_mut() {
                *s *= g;
            }
        }
    }
}

/// 电音：环形调制（金属边带）+ 原声混合 + 软削波。
/// carrier：载波 Hz；mix：ring 占比 0~1。免变调。
pub fn electric(pcm: &mut [f32], channels: u32, sample_rate: u32, carrier_hz: f32, mix: f32) {
    let ch = channels as usize;
    let frames = pcm.len() / ch;
    let w = 2.0 * std::f32::consts::PI * carrier_hz / sample_rate as f32;
    for f in 0..frames {
        let carrier = (f as f32 * w).sin();
        for c in 0..ch {
            let i = f * ch + c;
            let x = pcm[i];
            let ring = x * carrier;
            let y = mix * ring + (1.0 - mix) * x;
            pcm[i] = y;
        }
    }
    // 电音听感需要一点饱和把边带「推」出来
    for s in pcm.iter_mut() {
        *s = soft_clip(*s, 2.0);
    }
    normalize(pcm, 0.9);
}

/// 单二阶 biquad（RBJ cookbook），按通道独立扫（channel 交错布局）
pub struct Biquad {
    b0: f32, b1: f32, b2: f32, a1: f32, a2: f32,
    x1: f32, x2: f32, y1: f32, y2: f32,
}

impl Biquad {
    pub fn highpass(sample_rate: u32, f0: f32, q: f32) -> Self {
        let (b0, b1, b2, a1, a2) = rbj(sample_rate, f0, q, RbjKind::Highpass);
        Self { b0, b1, b2, a1, a2, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    pub fn lowpass(sample_rate: u32, f0: f32, q: f32) -> Self {
        let (b0, b1, b2, a1, a2) = rbj(sample_rate, f0, q, RbjKind::Lowpass);
        Self { b0, b1, b2, a1, a2, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    /// 对交错 PCM 的指定通道就地滤波（状态跨采样保持）
    pub fn process_channel(&mut self, pcm: &mut [f32], channels: u32, channel: usize) {
        let ch = channels as usize;
        for i in (channel..pcm.len()).step_by(ch) {
            let x = pcm[i];
            let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1 - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x;
            self.y2 = self.y1;
            self.y1 = y;
            pcm[i] = y;
        }
    }
}

enum RbjKind { Lowpass, Highpass }

/// RBJ Audio EQ Cookbook 公式（peaking 之外的 LP/HP 一阶推导）
fn rbj(sample_rate: u32, f0: f32, q: f32, kind: RbjKind) -> (f32, f32, f32, f32, f32) {
    let fs = sample_rate as f32;
    let f0 = f0.clamp(1.0, fs * 0.49);
    let w0 = 2.0 * std::f32::consts::PI * f0 / fs;
    let alpha = w0.sin() / (2.0 * q);
    let cos_w0 = w0.cos();
    let (b0, b1, b2, a0, a1, a2) = match kind {
        RbjKind::Lowpass => {
            let b1 = 1.0 - cos_w0;
            let b0 = b1 / 2.0;
            (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
        }
        RbjKind::Highpass => {
            let b1 = -(1.0 + cos_w0);
            let b0 = -b1 / 2.0;
            (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
        }
    };
    (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// 电话音：300–3400 Hz 带通（HP+LP 各两级级联 = 4 阶）+ 降采样到 ~8kHz 再升回
/// （sample-and-hold，「对讲机」数码感）+ 轻削波。免变调。
pub fn telephone(pcm: &mut [f32], channels: u32, sample_rate: u32) {
    let hp1 = Biquad::highpass(sample_rate, 300.0, 0.707);
    let hp2 = Biquad::highpass(sample_rate, 300.0, 0.707);
    let lp1 = Biquad::lowpass(sample_rate, 3_400.0, 0.707);
    let lp2 = Biquad::lowpass(sample_rate, 3_400.0, 0.707);
    let mut filters = [hp1, lp1, hp2, lp2];
    for c in 0..channels as usize {
        for filt in filters.iter_mut() {
            filt.process_channel(pcm, channels, c);
        }
    }
    // 降采样提升「数码感」：以 8kHz 网格采样保持（帧粒度）
    let target = 8_000u32.min(sample_rate);
    if target < sample_rate {
        let ch = channels as usize;
        let frames = pcm.len() / ch;
        let step = sample_rate as f32 / target as f32;
        let mut held: Vec<f32> = vec![0.0; ch];
        let mut last_src: usize = usize::MAX;
        for f in 0..frames {
            let src = ((f as f32 * step) as usize).min(frames - 1);
            if src != last_src {
                last_src = src;
                for c in 0..ch {
                    held[c] = pcm[src * ch + c];
                }
            }
            for c in 0..ch {
                pcm[f * ch + c] = held[c];
            }
        }
    }
    for s in pcm.iter_mut() {
        *s = soft_clip(*s, 1.6);
    }
    normalize(pcm, 0.85);
}

/// 失真电子声：tanh 削波 + 轻低通柔化毛刺 + 归一。免变调。
pub fn distorted(pcm: &mut [f32], channels: u32, sample_rate: u32, drive: f32) {
    for s in pcm.iter_mut() {
        *s = soft_clip(*s, drive);
    }
    let mut lp = Biquad::lowpass(sample_rate, 6_000.0, 0.707);
    for c in 0..channels as usize {
        lp.process_channel(pcm, channels, c);
    }
    normalize(pcm, 0.9);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成 n 采样 freq Hz 正弦（幅度 amp）
    fn sine(n: usize, freq: f32, fs: u32, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / fs as f32).sin())
            .collect()
    }

    fn rms(pcm: &[f32]) -> f32 {
        (pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len().max(1) as f32).sqrt()
    }

    #[test]
    fn 软削波_有界且单调() {
        for x in [-2.0f32, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0] {
            let y = soft_clip(x, 3.0);
            assert!(y.abs() < 1.0);
        }
        assert_eq!(soft_clip(0.0, 3.0), 0.0);
    }

    #[test]
    fn 归一_峰值到目标且静音保持() {
        let mut a = vec![0.5, -0.25, 0.1];
        normalize(&mut a, 0.9);
        assert!((a[0] - 0.9).abs() < 1e-6);
        let mut silent = vec![0.0f32; 100];
        normalize(&mut silent, 0.9);
        assert!(silent.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn 参数解析_字符串与数字兼容() {
        assert_eq!(param_f64(r#"{"freq":"55"}"#, "freq", 1.0), 55.0);
        assert_eq!(param_f64(r#"{"freq":55}"#, "freq", 1.0), 55.0);
        assert_eq!(param_f64(r#"{}"#, "freq", 7.0), 7.0);
        assert_eq!(param_f64("not json", "freq", 7.0), 7.0);
    }

    #[test]
    fn 电音_静音入静音出_有界() {
        let mut silent = vec![0.0f32; 4_800];
        electric(&mut silent, 1, 24_000, 65.0, 0.65);
        assert!(silent.iter().all(|s| s.abs() < 1e-9));
        let mut x = sine(24_000, 220.0, 24_000, 0.5);
        electric(&mut x, 1, 24_000, 65.0, 0.65);
        assert!(x.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn 带通_通带保留_带外衰减() {
        // 500 Hz 落在 300–3400 通带内应基本保留；50 Hz 应被大幅衰减
        let fs = 24_000u32;
        let mut inband = sine(fs as usize, 500.0, fs, 0.5);
        telephone(&mut inband, 1, fs);
        assert!(rms(&inband[fs as usize / 4..]) > 0.2, "通带能量被过度衰减");
        let mut low = sine(fs as usize, 50.0, fs, 0.5);
        telephone(&mut low, 1, fs);
        assert!(rms(&low[fs as usize / 4..]) < 0.08, "低频未被抑制");
    }

    #[test]
    fn 降采样后长度不变() {
        let fs = 24_000u32;
        let mut x = sine(fs as usize, 440.0, fs, 0.5);
        let n = x.len();
        telephone(&mut x, 1, fs);
        assert_eq!(x.len(), n);
    }

    #[test]
    fn 失真_输出限幅() {
        let mut x = sine(4_800, 220.0, 24_000, 0.9);
        distorted(&mut x, 1, 24_000, 5.0);
        assert!(x.iter().all(|s| s.abs() <= 1.0));
        assert!((x.iter().fold(0f32, |m, s| m.max(s.abs())) - 0.9).abs() < 1e-4);
    }

    #[test]
    fn 立体声_帧相位一致() {
        // 同一信号进左右通道，电音输出两通道必须一致（carrier 按帧计算）
        let fs = 24_000u32;
        let mono = sine(2_400, 220.0, fs, 0.5);
        let mut stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        electric(&mut stereo, 2, fs, 65.0, 0.65);
        for f in 0..2_400 {
            assert!((stereo[f * 2] - stereo[f * 2 + 1]).abs() < 1e-6);
        }
    }
}

/// granular 音高移位（变调不变速）：重叠加窗（hann，75% overlap）+ 变步长读指针。
/// semitones：正=升调（花栗鼠），负=降调（低音炮）。输出长度与输入严格一致；
/// 纯正弦输入不产生直流漂移（两钉见测试）。
pub fn pitch_shift(pcm: &mut [f32], channels: u32, sample_rate: u32, semitones: f32) {
    if semitones.abs() < 1e-3 || channels == 0 {
        return;
    }
    let ratio = 2f32.powf(semitones / 12.0);
    let ch = channels as usize;
    let frames = pcm.len() / ch;
    if frames == 0 {
        return;
    }
    // grain ≈ 46ms（按采样率自适应），hop = grain/4（hann 75% overlap 满足 COLA）
    let n = ((sample_rate as f64 * 0.046) as usize).clamp(128, 8192);
    let hop = (n / 4).max(1);
    let win: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1).max(1) as f32;
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * t).cos()
        })
        .collect();

    for c in 0..ch {
        let mut out = vec![0f32; frames];
        let mut norm = vec![0f32; frames];
        let mut read = 0f32; // 本 grain 的输入起点（帧号）；与输出同步推进 → 时长不变
        let mut grain_out = 0usize;
        while grain_out < frames {
            for i in 0..n {
                let o = grain_out + i;
                if o >= frames {
                    break;
                }
                let r = read + i as f32 * ratio; // grain 内以 ratio 读 → 音高 ×ratio
                let ri = r.floor();
                if ri >= 0.0 {
                    let ri = ri as usize;
                    if ri + 1 < frames {
                        let frac = r - ri as f32;
                        let s = pcm[ri * ch + c] * (1.0 - frac) + pcm[(ri + 1) * ch + c] * frac;
                        out[o] += s * win[i];
                    }
                }
                norm[o] += win[i];
            }
            grain_out += hop;
            read += hop as f32;
        }
        // 窗和归一：消除 OLA 幅度纹波；未覆盖区（头尾）静音
        for f in 0..frames {
            pcm[f * ch + c] = if norm[f] > 1e-6 { out[f] / norm[f] } else { 0.0 };
        }
    }
}

#[cfg(test)]
mod pitch_tests {
    use super::*;

    fn sine(n: usize, freq: f32, fs: u32, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / fs as f32).sin())
            .collect()
    }

    /// 数过零点估频率（整段）
    fn zero_cross_freq(pcm: &[f32], fs: u32) -> f32 {
        let mut crossings = 0usize;
        for w in pcm.windows(2) {
            if w[0] < 0.0 && w[1] >= 0.0 {
                crossings += 1;
            }
        }
        crossings as f32 * fs as f32 / pcm.len() as f32
    }

    #[test]
    fn 钉一_输出长度与输入一致() {
        for &semis in &[4.0f32, -4.0, 7.0, -1.0] {
            let mut x = sine(48_000, 440.0, 24_000, 0.5);
            let n = x.len();
            pitch_shift(&mut x, 1, 24_000, semis);
            assert_eq!(x.len(), n, "semitones={semis} 长度不得变化");
        }
    }

    #[test]
    fn 钉二_纯正弦无直流漂移() {
        for &semis in &[4.0f32, -4.0] {
            let mut x = sine(48_000, 440.0, 24_000, 0.5);
            pitch_shift(&mut x, 1, 24_000, semis);
            let mean = x.iter().sum::<f32>() / x.len() as f32;
            assert!(mean.abs() < 0.01, "semitones={semis} 直流漂移 {mean}");
        }
    }

    #[test]
    fn 变调方向正确_过零率验证() {
        let fs = 24_000u32;
        let mut up = sine(fs as usize, 440.0, fs, 0.5);
        pitch_shift(&mut up, 1, fs, 4.0);
        let f_up = zero_cross_freq(&up[2_000..fs as usize - 2_000], fs);
        assert!(
            (f_up - 440.0 * 2f32.powf(4.0 / 12.0)).abs() < 55.0,
            "+4 半音应为 ~554Hz，实测 {f_up}"
        );
        let mut down = sine(fs as usize, 440.0, fs, 0.5);
        pitch_shift(&mut down, 1, fs, -4.0);
        let f_down = zero_cross_freq(&down[2_000..fs as usize - 2_000], fs);
        assert!(
            (f_down - 440.0 * 2f32.powf(-4.0 / 12.0)).abs() < 45.0,
            "-4 半音应为 ~349Hz，实测 {f_down}"
        );
    }

    #[test]
    fn 静音入静音出_峰值有界() {
        let mut silent = vec![0.0f32; 24_000];
        pitch_shift(&mut silent, 1, 24_000, 4.0);
        assert!(silent.iter().all(|s| *s == 0.0));
        let mut x = sine(24_000, 440.0, 24_000, 0.5);
        pitch_shift(&mut x, 1, 24_000, 4.0);
        assert!(x.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn 立体声_长度与通道一致() {
        let fs = 24_000u32;
        let mono = sine(9_600, 440.0, fs, 0.5);
        let mut stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        pitch_shift(&mut stereo, 2, fs, 4.0);
        assert_eq!(stereo.len(), 9_600 * 2);
        for f in 4_800..9_600 {
            assert!(
                (stereo[f * 2] - stereo[f * 2 + 1]).abs() < 1e-5,
                "同源双通道输出应一致"
            );
        }
    }
}
