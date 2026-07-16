// 数据结构定义 + 时间/id 工具
// 三份数据文件（messages/favorites/settings）的 Rust 表示。

use serde::{Deserialize, Serialize};

/// 一条消息记录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// 唯一 id，形如 "m_<毫秒时间戳>_<前4位随机>", 避免双窗口同毫秒撞车
    pub id: String,
    /// 输入的文本内容
    pub content: String,
    /// 音频相对路径，相对 app_data_dir，形如 "audio/m_xxx.mp3"
    pub audio_path: String,
    /// ISO8601 时间戳字符串，如 "2026-07-12T18:00:00+08:00"
    pub created_at: String,
}

/// 一条收藏记录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Favorite {
    /// 唯一 id，形如 "f_<毫秒时间戳>_<前4位随机>"
    pub id: String,
    /// 来源消息 id；来源消息被删除时置 None（收藏本身仍保留）
    pub source_message_id: Option<String>,
    /// 备注（必填，非空字符串）
    pub note: String,
    /// 音频相对路径
    pub audio_path: String,
    /// ISO8601 时间戳字符串
    pub created_at: String,
}

/// 设置（settings.json 的完整结构）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// 当前引擎名（如 "mimo"）
    pub tts_engine: String,
    /// 引擎内的模型/音色 id
    pub tts_model: String,
    /// 播放音量 0.0~1.0（前端控制）
    pub playback_volume: f32,
    /// 呼出浮窗的全局快捷键（如 "Alt+V"）
    pub hotkey_show_window: String,
    /// 引擎类别 "remote" / "local"
    pub engine_category: String,
    /// MiMo TTS API Key（明文存，settings.json 中的 mimo_api_key）
    pub mimo_api_key: String,
    /// 播放速度 0.5~2.0（前端 HTMLAudioElement.playbackRate，精确控制）
    pub playback_rate: f32,
    /// 克隆音色起的名字（空字符串 = 无克隆样本）
    pub clone_voice_name: String,
    /// 克隆音色样本相对路径，如 "voice_samples/clone.mp3"（空字符串 = 无）
    pub clone_voice_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tts_engine: "mimo".to_string(),
            tts_model: "default".to_string(),
            playback_volume: 0.8,
            hotkey_show_window: "Alt+V".to_string(),
            engine_category: "remote".to_string(),
            mimo_api_key: String::new(),
            playback_rate: 1.0,
            clone_voice_name: String::new(),
            clone_voice_path: String::new(),
        }
    }
}

/// 生成全局唯一 id。
///
/// 用毫秒时间戳 + 进程内计数器，避免同一毫秒内多次调用撞车。
/// 前缀区分类型："m_" 消息 / "f_" 收藏。
///
/// 用户不懂技术的话解释：时间戳保证不同时刻不会重复，
/// 计数器保证同一毫秒内的两次调用也不会重复（主窗+浮窗同时发的极端场景）。
pub fn gen_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = chrono::Utc::now().timestamp_millis();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}_{ms}_{n}")
}

/// 生成当前时间的 ISO8601 字符串（带本地时区偏移），用于 created_at。
pub fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_id_前缀正确且递增不重复() {
        let a = gen_id("m");
        let b = gen_id("m");
        assert!(a.starts_with("m_"));
        assert!(b.starts_with("m_"));
        assert_ne!(a, b, "连续两次 gen_id 不得重复");
    }

    #[test]
    fn gen_id_不同前缀共存() {
        let m = gen_id("m");
        let f = gen_id("f");
        assert!(m.starts_with("m_"));
        assert!(f.starts_with("f_"));
    }

    #[test]
    fn settings_默认值合理() {
        let s = Settings::default();
        assert_eq!(s.tts_engine, "mimo");
        assert!((s.playback_volume - 0.8).abs() < 1e-6);
        assert_eq!(s.hotkey_show_window, "Alt+V");
    }

    #[test]
    fn settings_可序列化往返() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn now_iso_格式合法() {
        let s = now_iso();
        // 至少能再解析回来，说明是合规的 RFC3339
        let _ = chrono::DateTime::parse_from_rfc3339(&s).expect("now_iso 必须是合规 RFC3339");
    }
}