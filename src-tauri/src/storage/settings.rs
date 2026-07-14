// settings.json 读写
//
// 设计：
// - load_settings：文件不存在或部分字段缺失时，用默认值补齐返回（容错）
// - save_settings：整体写回
// - update_setting：改单个键返回完整 settings（靠 serde 重建对象实现"不丢别的键"）

use std::path::Path;
use super::atomic::{write_json_pretty, Result};
use super::types::Settings;

const FILE: &str = "settings.json";

fn path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(FILE)
}

/// 读设置。文件不存在/解析失败/部分字段缺失时用默认值补齐，永不报错。
/// 这样老版本配置文件升级时，缺的新字段会自动用默认值兜住。
pub fn load_settings(data_dir: &Path) -> Settings {
    let p = path(data_dir);
    if !p.exists() {
        return Settings::default();
    }
    let raw = match std::fs::read_to_string(&p) {
        Ok(s) => s,
        Err(_) => return Settings::default(),
    };
    // 解析成 Value，缺失字段用默认值一层层补
    let default = Settings::default();
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => Settings {
            tts_engine: v.get("tts_engine").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.tts_engine),
            tts_model: v.get("tts_model").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.tts_model),
            playback_volume: v.get("playback_volume").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(default.playback_volume),
            hotkey_show_window: v.get("hotkey_show_window").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.hotkey_show_window),
            engine_category: v.get("engine_category").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.engine_category),
            mimo_api_key: v.get("mimo_api_key").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.mimo_api_key),
            playback_rate: v.get("playback_rate").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(default.playback_rate),
        },
        Err(_) => default,
    }
}

/// 整体写回设置。
pub fn save_settings(data_dir: &Path, settings: &Settings) -> Result<()> {
    write_json_pretty(&path(data_dir), settings)
}

/// 改单个键并返回完整的 settings。key 为已知的几个字段名。
pub fn update_setting(data_dir: &Path, key: &str, value: serde_json::Value) -> Result<Settings> {
    let mut s = load_settings(data_dir);
    match key {
        "tts_engine" => if let Some(v) = value.as_str() { s.tts_engine = v.to_string() },
        "tts_model" => if let Some(v) = value.as_str() { s.tts_model = v.to_string() },
        "playback_volume" => if let Some(v) = value.as_f64() { s.playback_volume = v as f32 },
        "hotkey_show_window" => if let Some(v) = value.as_str() { s.hotkey_show_window = v.to_string() },
        "engine_category" => if let Some(v) = value.as_str() { s.engine_category = v.to_string() },
        "mimo_api_key" => if let Some(v) = value.as_str() { s.mimo_api_key = v.to_string() },
        "playback_rate" => if let Some(v) = value.as_f64() { s.playback_rate = v as f32 },
        _ => {} // 未知键忽略
    }
    save_settings(data_dir, &s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn 文件不存在返回默认值() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let s = load_settings(&d);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn 写入再读回往返() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let s = Settings { playback_volume: 0.3, ..Settings::default() };
        save_settings(&d, &s).unwrap();
        let got = load_settings(&d);
        assert_eq!(got, s);
    }

    #[test]
    fn 部分字段缺失用默认补齐() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        // 只写一个字段
        std::fs::write(d.join("settings.json"), r#"{"playback_volume":0.1}"#).unwrap();
        let s = load_settings(&d);
        assert!((s.playback_volume - 0.1).abs() < 1e-6, "保留写下的");
        assert_eq!(s.tts_engine, "mimo", "缺失字段用默认");
        assert_eq!(s.hotkey_show_window, "Alt+V");
    }

    #[test]
    fn 文件损坏返回默认() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        std::fs::write(d.join("settings.json"), "不是合法json{{").unwrap();
        let s = load_settings(&d);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn 增量改一个键不丢其它键() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let s = Settings { playback_volume: 0.5, hotkey_show_window: "Ctrl+Shift+V".into(), ..Settings::default() };
        save_settings(&d, &s).unwrap();
        let back = update_setting(&d, "playback_volume", serde_json::json!(0.9)).unwrap();
        assert!((back.playback_volume - 0.9).abs() < 1e-6, "新值写入");
        assert_eq!(back.hotkey_show_window, "Ctrl+Shift+V", "其它键保留");
    }

    #[test]
    fn 未知的键忽略不报错() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let _ = update_setting(&d, "不存在的键", serde_json::json!("x")).unwrap();
    }
}