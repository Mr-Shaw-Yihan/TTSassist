// 能量语音活动检测（VAD）+ 句子切割。
//
// 为什么用纯能量 VAD 而不引 webrtc-vad：
// - webrtc-vad 需要 C 依赖（编译链复杂、跨平台坑多），收益不匹配（我们要的不是
//   逐帧语音/非语音分类，而是「有人说话就把这段攒起来送 ASR，静音够久就切一句」）。
// - 系统混音里非语音（游戏 BGM、脚步、杂音）本来就多，靠能量阈值 + 最短语音时长
//   过滤已足够省钱；真正判断是不是能识别成文字交给 ASR 插件（返回空串就丢弃）。
//
// 工作方式：输入是连续的单声道 f32 采样（[-1,1]，16kHz）。每凑满一帧算 RMS，
// 高于阈值标记为语音活动，累积进当前句子缓冲；连续静音超过 hangover 且当前句子
// 够长，就产出一句（回调送出该句的 PCM），然后清空缓冲重新计。

use serde::{Deserialize, Serialize};

/// 目标帧长（秒）：20ms。16kHz 下 = 320 样本。
pub const FRAME_SECONDS: f32 = 0.020;

/// VAD 可调参数（可从 Settings 的敏感度档位映射而来）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfig {
    /// RMS 阈值：低于此值视为静音。
    pub rms_threshold: f32,
    /// 一帧内静音判定后，允许再忍受多长的连续静音才切句（秒）。
    pub hangover_seconds: f32,
    /// 一段语音至少持续多久才算「一句有效语音」（秒），短于此丢弃（防咔哒声误触发）。
    pub min_speech_seconds: f32,
    /// 一句最长不超过多久（秒），超过强制切，避免一直说个不停时缓冲无限涨。
    pub max_speech_seconds: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self::from_sensitivity("medium")
    }
}

impl VadConfig {
    /// 由敏感度档位映射参数。
    /// - low：阈值高，只有很响的语音才触发（最省，但可能漏小声）
    /// - medium：均衡（默认）
    /// - high：阈值低，小声也能抓（费钱，但游戏背景音里队友小声说话更稳）
    pub fn from_sensitivity(sensitivity: &str) -> Self {
        let rms_threshold = match sensitivity {
            "low" => 0.020,
            "high" => 0.007,
            _ => 0.012, // medium 及未知值
        };
        Self {
            rms_threshold,
            hangover_seconds: 0.75,
            min_speech_seconds: 0.20,
            max_speech_seconds: 12.0,
        }
    }
}

/// VAD 产出一句语音的回调入参：该句的原始单声道 f32 PCM（16kHz）。
pub struct SpeechSegment {
    pub samples: Vec<f32>,
}

/// 能量 VAD 状态机。逐帧喂样本，凑够一句时通过 `on_segment` 回调送出。
pub struct EnergyVad {
    cfg: VadConfig,
    sample_rate: u32,
    frame_size: usize,
    /// 当前正在累积的句子样本（含句首前置的一点缓冲，见 push 逻辑）
    buffer: Vec<f32>,
    /// 已缓冲样本对应的时长（秒）
    buffered_seconds: f32,
    /// 距离上一次「语音帧」已累计的静音时长（秒）
    silence_seconds: f32,
    /// 当前句子是否已经开始（遇到过语音帧）
    in_speech: bool,
}

impl EnergyVad {
    pub fn new(cfg: VadConfig, sample_rate: u32) -> Self {
        let frame_size = ((FRAME_SECONDS * sample_rate as f32).round() as usize).max(1);
        Self {
            cfg,
            sample_rate,
            frame_size,
            buffer: Vec::new(),
            buffered_seconds: 0.0,
            silence_seconds: 0.0,
            in_speech: false,
        }
    }

    /// 喂入一段样本（[-1,1] 单声道 f32），凑满整帧则逐帧处理。
    /// 每当产出一句完整语音，调用 `on_segment`。
    ///
    /// `leftover` 是跨调用的残余样本（凑不满一帧的尾巴），由调用方持有并回传，
    /// 保证连续喂数据时帧边界对齐。
    pub fn push(&mut self, samples: &[f32], leftover: &mut Vec<f32>, on_segment: &mut dyn FnMut(SpeechSegment)) {
        // 先把上次的残余和新数据拼起来
        let mut iter_data: Vec<f32> = std::mem::take(leftover);
        iter_data.extend_from_slice(samples);

        let mut pos = 0usize;
        while pos + self.frame_size <= iter_data.len() {
            let frame = &iter_data[pos..pos + self.frame_size];
            pos += self.frame_size;
            self.process_frame(frame, on_segment);
        }

        // 不足一帧的尾巴留到下次
        leftover.clear();
        leftover.extend_from_slice(&iter_data[pos..]);
    }

    /// 处理一帧，返回该帧是否为语音帧（供单测与内部使用）。
    fn process_frame(&mut self, frame: &[f32], on_segment: &mut dyn FnMut(SpeechSegment)) -> bool {
        let frame_seconds = frame.len() as f32 / self.sample_rate as f32;
        let is_voice = rms(frame) >= self.cfg.rms_threshold;

        if is_voice {
            if !self.in_speech {
                // 句子刚开始：之前的静音不缓冲（丢弃以省内存），从此帧起攒
                self.in_speech = true;
                self.buffer.clear();
                self.buffered_seconds = 0.0;
            }
            self.silence_seconds = 0.0;
            self.buffer.extend_from_slice(frame);
            self.buffered_seconds += frame_seconds;

            // 超长强制切（防止一直不停说导致缓冲爆炸）
            if self.buffered_seconds >= self.cfg.max_speech_seconds {
                self.flush(on_segment, true);
            }
        } else if self.in_speech {
            // 已在句子中遇到静音：仍攒进缓冲（保留词间停顿），并累计静音时长
            self.buffer.extend_from_slice(frame);
            self.buffered_seconds += frame_seconds;
            self.silence_seconds += frame_seconds;

            if self.silence_seconds >= self.cfg.hangover_seconds {
                // 静音够久 → 判句尾，尝试出句
                self.flush(on_segment, false);
            } else if self.buffered_seconds >= self.cfg.max_speech_seconds {
                self.flush(on_segment, true);
            }
        }
        // 若 !is_voice && !in_speech：完全静音，啥也不做（不缓冲）
        is_voice
    }

    /// 结束当前句子：够长则产出，否则丢弃。`forced=true` 表示因超长强制切。
    fn flush(&mut self, on_segment: &mut dyn FnMut(SpeechSegment), _forced: bool) {
        // 剥掉尾部 hangover 静音对判定无影响，用「总时长 - 静音时长」近似语音时长；
        // 简化：只要缓冲总时长 ≥ min_speech + hangover 即认为有足够语音。
        // （hangover 那段静音已被计入缓冲，真正的语音至少要有 min_speech_seconds）
        let speech_seconds = (self.buffered_seconds - self.silence_seconds).max(0.0);
        if self.in_speech && speech_seconds >= self.cfg.min_speech_seconds {
            // 去掉尾部 hangover 静音，只送语音部分（省 ASR 流量、去尾噪）
            let tail_silence_samples =
                ((self.silence_seconds * self.sample_rate as f32) as usize).min(self.buffer.len());
            let keep = self.buffer.len() - tail_silence_samples;
            let mut samples = std::mem::take(&mut self.buffer);
            samples.truncate(keep);
            if !samples.is_empty() {
                on_segment(SpeechSegment { samples });
            }
        }
        self.buffer.clear();
        self.buffered_seconds = 0.0;
        self.silence_seconds = 0.0;
        self.in_speech = false;
    }

    /// 强制收束当前正在累积的句子（停止监听时调用，避免末句丢失）。
    pub fn finish(&mut self, on_segment: &mut dyn FnMut(SpeechSegment)) {
        if self.in_speech {
            self.silence_seconds = 0.0; // 末句不算尾部静音
            self.flush(on_segment, true);
        }
    }
}

/// 计算一帧的均方根（RMS）。空帧返回 0。
pub fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f32 = frame.iter().map(|s| s * s).sum();
    (sum / frame.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16000;

    /// 生成一段恒定振幅、给定频率的正弦样本
    fn tone(freq: f32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (seconds * SR as f32) as usize;
        (0..n)
            .map(|i| {
                amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin()
            })
            .collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SR as f32) as usize]
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn rms_of_full_scale_square_is_one() {
        let frame = vec![1.0; 320];
        assert!((rms(&frame) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn single_speech_burst_produces_one_segment() {
        let cfg = VadConfig::from_sensitivity("medium");
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut segments = Vec::new();

        // 0.5s 语音（0.3 振幅，远高于 medium 阈值 0.012）+ 1s 静音触发切句
        vad.push(&tone(200.0, 0.5, 0.3), &mut leftover, &mut |seg| {
            segments.push(seg.samples.len())
        });
        vad.push(&silence(1.0), &mut leftover, &mut |seg| {
            segments.push(seg.samples.len())
        });

        assert_eq!(segments.len(), 1, "应恰好产出一句");
        // 0.5s @ 16k = 8000 样本，尾部 hangover 静音被剥离，故略多于 8000 附近但少于全静音长度
        assert!(segments[0] >= 8000, "句子样本数应覆盖语音时长，实际 {}", segments[0]);
    }

    #[test]
    fn short_click_below_min_speech_is_dropped() {
        let cfg = VadConfig::from_sensitivity("medium");
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut count = 0;

        // 仅 50ms 语音（< min_speech 200ms），随后长静音
        vad.push(&tone(200.0, 0.05, 0.3), &mut leftover, &mut |_| count += 1);
        vad.push(&silence(1.0), &mut leftover, &mut |_| count += 1);

        assert_eq!(count, 0, "短于 min_speech 的咔哒声应被丢弃");
    }

    #[test]
    fn two_bursts_produce_two_segments() {
        let cfg = VadConfig::from_sensitivity("medium");
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut count = 0;
        let mut on = |seg: SpeechSegment| {
            let _ = seg;
            count += 1;
        };

        vad.push(&tone(200.0, 0.4, 0.3), &mut leftover, &mut on);
        vad.push(&silence(1.0), &mut leftover, &mut on);
        vad.push(&tone(300.0, 0.4, 0.3), &mut leftover, &mut on);
        vad.push(&silence(1.0), &mut leftover, &mut on);

        assert_eq!(count, 2, "两段分离语音应产出两句");
    }

    #[test]
    fn silence_above_threshold_triggers_nothing() {
        // 振幅远低于 low 档阈值 0.020 的微噪声不应触发
        let cfg = VadConfig::from_sensitivity("low");
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut count = 0;

        vad.push(&tone(200.0, 2.0, 0.005), &mut leftover, &mut |_| count += 1);
        vad.push(&silence(1.0), &mut leftover, &mut |_| count += 1);

        assert_eq!(count, 0, "低于阈值的微噪声不应成句");
    }

    #[test]
    fn finish_emits_incomplete_trailing_speech() {
        let cfg = VadConfig::from_sensitivity("medium");
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut count = 0;

        // 语音还没遇到 hangover 静音就结束监听
        vad.push(&tone(200.0, 0.6, 0.3), &mut leftover, &mut |_| count += 1);
        assert_eq!(count, 0, "未达 hangover 时不应自动出句");
        vad.finish(&mut |_| count += 1);
        assert_eq!(count, 1, "finish 应强制收束末句");
    }

    #[test]
    fn long_speech_force_cut_at_max() {
        let mut cfg = VadConfig::from_sensitivity("medium");
        cfg.max_speech_seconds = 1.0; // 便于测试：1s 强制切
        let mut vad = EnergyVad::new(cfg, SR);
        let mut leftover = Vec::new();
        let mut count = 0;

        // 连续 2.5s 语音无停顿 → 应在 1s、2s 处各强切一句
        vad.push(&tone(200.0, 2.5, 0.3), &mut leftover, &mut |_| count += 1);
        assert!(count >= 2, "超长语音应按 max 强制切多句，实际 {count}");
    }
}
