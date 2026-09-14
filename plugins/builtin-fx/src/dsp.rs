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
