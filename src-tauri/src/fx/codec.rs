// 效果器编解码：rodio（symphonia）解码任意引擎产物为交错 PCM，
// hound 编码 16-bit WAV 落盘缓存。宿主管编解码、插件只做 DSP（设计 §四）。
//
// 注：rodio 0.20 的 Decoder 统一产出 i16（各格式一致），此处归一化为
// 交错 f32（[-1,1]）再交给效果器插件，处理后仍由 hound 以 16-bit WAV 落盘。

use std::io::BufReader;
use std::path::Path;

/// 解码音频文件为交错 f32 PCM（支持 rodio 认的格式：wav/mp3/flac/ogg 等）。
/// 返回 (交错 PCM, 采样率, 通道数)。
pub fn decode_file_f32(path: &Path) -> Result<(Vec<f32>, u32, u32), String> {
    use rodio::Source;

    let file = std::fs::File::open(path).map_err(|e| format!("打开音频失败：{e}"))?;
    let source = rodio::Decoder::new(BufReader::new(file))
        .map_err(|e| format!("音频解码失败（仅支持 WAV/MP3/FLAC/OGG）：{e}"))?;
    let sample_rate = source.sample_rate();
    let channels = u32::from(source.channels());
    let mut pcm = Vec::new();
    for s in source {
        pcm.push(f32::from(s) / 32_768.0);
    }
    if sample_rate == 0 || channels == 0 {
        return Err("音频元信息异常（采样率或通道数为 0）".into());
    }
    Ok((pcm, sample_rate, channels))
}

/// 交错 f32 PCM 编码为 16-bit PCM WAV（限幅 [-1,1] 后写 i16）
pub fn encode_wav16(
    path: &Path,
    pcm: &[f32],
    sample_rate: u32,
    channels: u32,
) -> Result<(), String> {
    let Ok(ch) = u16::try_from(channels) else {
        return Err(format!("非法通道数 {channels}"));
    };
    let spec = hound::WavSpec {
        channels: ch,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("创建 WAV 失败：{e}"))?;
    for &s in pcm {
        let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
        writer.write_sample(v).map_err(|e| format!("写入 WAV 失败：{e}"))?;
    }
    writer.finalize().map_err(|e| format!("完成 WAV 失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 编码 → 解码往返：长度/采样率/通道数保持，样值在 16-bit 精度内还原
    #[test]
    fn wav编码解码往返() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("round.wav");
        let sample_rate = 24_000u32;
        let channels = 1u32;
        let src: Vec<f32> = (0..2400)
            .map(|i| (i as f32 * 0.01).sin() * 0.5)
            .collect();
        encode_wav16(&path, &src, sample_rate, channels).unwrap();
        let (pcm, sr, ch) = decode_file_f32(&path).unwrap();
        assert_eq!(sr, sample_rate);
        assert_eq!(ch, channels);
        assert_eq!(pcm.len(), src.len());
        for (a, b) in src.iter().zip(pcm.iter()) {
            assert!((a - b).abs() < 2.0 / 32_767.0 + 1e-6);
        }
    }

    #[test]
    fn 编码限幅不越界() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        encode_wav16(&path, &[2.0, -2.0, 0.0], 8_000, 1).unwrap();
        let (pcm, _, _) = decode_file_f32(&path).unwrap();
        assert!(pcm[0] <= 1.0 && pcm[1] >= -1.0);
        assert_eq!(pcm[2], 0.0);
    }
}
