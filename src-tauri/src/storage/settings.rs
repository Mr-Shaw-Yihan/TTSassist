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
    let mut settings = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => Settings {
            tts_engine: v.get("tts_engine").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.tts_engine),
            tts_model: v.get("tts_model").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.tts_model),
            playback_volume: v.get("playback_volume").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(default.playback_volume),
            hotkey_show_window: v.get("hotkey_show_window").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.hotkey_show_window),
            engine_category: v.get("engine_category").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.engine_category),
            mimo_api_key: v.get("mimo_api_key").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.mimo_api_key),
            playback_rate: v.get("playback_rate").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(default.playback_rate),
            clone_voice_name: v.get("clone_voice_name").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.clone_voice_name),
            clone_voice_path: v.get("clone_voice_path").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.clone_voice_path),
            theme: v.get("theme").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.theme),
            moss_api_key: v.get("moss_api_key").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.moss_api_key),
            minimax_api_key: v.get("minimax_api_key").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.minimax_api_key),
            minimax_global_api_key: v.get("minimax_global_api_key").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.minimax_global_api_key),
            minimax_global_cloned_voices: v.get("minimax_global_cloned_voices")
                .and_then(|x| serde_json::from_value::<Vec<String>>(x.clone()).ok())
                .unwrap_or(default.minimax_global_cloned_voices),
            moss_voice_id: v.get("moss_voice_id").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.moss_voice_id),
            moss_voices: v.get("moss_voices")
                .and_then(|x| serde_json::from_value::<Vec<super::types::MossVoice>>(x.clone()).ok())
                .unwrap_or(default.moss_voices),
            mic_output_device: v.get("mic_output_device").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.mic_output_device),
            mic_send_enabled: v.get("mic_send_enabled").and_then(|x| x.as_bool()).unwrap_or(default.mic_send_enabled),
            mic_playback_volume: v.get("mic_playback_volume").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(default.mic_playback_volume),
            plugin_voices: v.get("plugin_voices")
                .and_then(|x| serde_json::from_value::<std::collections::HashMap<String, String>>(x.clone()).ok())
                .unwrap_or(default.plugin_voices),
            update_ignored_version: v.get("update_ignored_version").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.update_ignored_version),
            asr_plugin: v.get("asr_plugin").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.asr_plugin),
            asr_language: v.get("asr_language").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.asr_language),
            voice_input_hotkey: v.get("voice_input_hotkey").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.voice_input_hotkey),
            voice_input_enabled: v.get("voice_input_enabled").and_then(|x| x.as_bool()).unwrap_or(default.voice_input_enabled),
            voice_input_device: v.get("voice_input_device").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.voice_input_device),
            hotkey_play_last: v.get("hotkey_play_last").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.hotkey_play_last),
            hotkey_mic_toggle: v.get("hotkey_mic_toggle").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.hotkey_mic_toggle),
            plugin_config: v.get("plugin_config")
                .and_then(|x| serde_json::from_value::<std::collections::HashMap<String, std::collections::HashMap<String, String>>>(x.clone()).ok())
                .unwrap_or(default.plugin_config),
            floating_ball_enabled: v.get("floating_ball_enabled").and_then(|x| x.as_bool()).unwrap_or(default.floating_ball_enabled),
            floating_ball_x: v.get("floating_ball_x").and_then(|x| x.as_i64()).map(|x| x as i32).unwrap_or(default.floating_ball_x),
            floating_ball_y: v.get("floating_ball_y").and_then(|x| x.as_i64()).map(|x| x as i32).unwrap_or(default.floating_ball_y),
            floating_ball_size: v.get("floating_ball_size").and_then(|x| x.as_i64()).map(|x| super::types::clamp_ball_size(x as i32)).unwrap_or(default.floating_ball_size),
            floating_ball_perf_mode: v.get("floating_ball_perf_mode").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.floating_ball_perf_mode),
            floating_ball_skin: v.get("floating_ball_skin").and_then(|x| x.as_str()).map(String::from).unwrap_or(default.floating_ball_skin),
        },
        Err(_) => default,
    };
    // 存量迁移：旧顶层 API Key 键搬入 plugin_config（一次性，搬完落盘删旧键）
    if migrate_legacy_plugin_keys(&mut settings) {
        let _ = save_settings(data_dir, &settings);
    }
    settings
}

/// 旧硬编码插件 Key → plugin_config 的迁移映射。
/// 一次性历史包袱清理代码：宿主删除 minimax 硬编码两个版本后可删。
const LEGACY_PLUGIN_KEY_MAP: &[(&str, &str, &str)] = &[
    // (settings 旧键, 插件 id, plugin_config 字段 key)
    ("minimax_api_key", "minimax-tts", "api_key"),
    ("minimax_global_api_key", "minimax-tts-global", "api_key"),
];

/// 把旧顶层 Key 键搬入 plugin_config 对应条目并清空旧键。
/// 仅当旧键非空且新位置为空时搬入（不覆盖用户已在新面板填写的值）。
/// 返回是否发生了改动（调用方据此落盘）。
fn migrate_legacy_plugin_keys(s: &mut Settings) -> bool {
    let mut changed = false;
    for (legacy_key, plugin_id, field_key) in LEGACY_PLUGIN_KEY_MAP {
        let legacy_value = match *legacy_key {
            "minimax_api_key" => s.minimax_api_key.clone(),
            "minimax_global_api_key" => s.minimax_global_api_key.clone(),
            _ => String::new(),
        };
        if legacy_value.is_empty() {
            continue;
        }
        let empty_here = s
            .plugin_config
            .get(*plugin_id)
            .and_then(|m| m.get(*field_key))
            .map_or(true, |v| v.is_empty());
        if empty_here {
            s.plugin_config
                .entry(plugin_id.to_string())
                .or_default()
                .insert(field_key.to_string(), legacy_value);
        }
        // 无论是否搬入（新位置已有值时旧值作废），旧键都清空
        match *legacy_key {
            "minimax_api_key" => s.minimax_api_key = String::new(),
            "minimax_global_api_key" => s.minimax_global_api_key = String::new(),
            _ => {}
        }
        changed = true;
    }
    changed
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
        "clone_voice_name" => if let Some(v) = value.as_str() { s.clone_voice_name = v.to_string() },
        "clone_voice_path" => if let Some(v) = value.as_str() { s.clone_voice_path = v.to_string() },
        "theme" => if let Some(v) = value.as_str() { s.theme = v.to_string() },
        "moss_api_key" => if let Some(v) = value.as_str() { s.moss_api_key = v.to_string() },
        "minimax_api_key" => if let Some(v) = value.as_str() { s.minimax_api_key = v.to_string() },
        "minimax_global_api_key" => if let Some(v) = value.as_str() { s.minimax_global_api_key = v.to_string() },
        "minimax_global_cloned_voices" => if let Ok(list) = serde_json::from_value::<Vec<String>>(value.clone()) { s.minimax_global_cloned_voices = list },
        "moss_voice_id" => if let Some(v) = value.as_str() { s.moss_voice_id = v.to_string() },
        "moss_voices" => if let Ok(list) = serde_json::from_value::<Vec<super::types::MossVoice>>(value.clone()) { s.moss_voices = list },
        "mic_output_device" => if let Some(v) = value.as_str() { s.mic_output_device = v.to_string() },
        "mic_send_enabled" => if let Some(v) = value.as_bool() { s.mic_send_enabled = v },
        "mic_playback_volume" => if let Some(v) = value.as_f64() { s.mic_playback_volume = v as f32 },
        "plugin_voices" => if let Ok(map) = serde_json::from_value::<std::collections::HashMap<String, String>>(value.clone()) { s.plugin_voices = map },
        "update_ignored_version" => if let Some(v) = value.as_str() { s.update_ignored_version = v.to_string() },
        "asr_plugin" => if let Some(v) = value.as_str() { s.asr_plugin = v.to_string() },
        "asr_language" => if let Some(v) = value.as_str() { s.asr_language = v.to_string() },
        "voice_input_hotkey" => if let Some(v) = value.as_str() { s.voice_input_hotkey = v.to_string() },
        "voice_input_enabled" => if let Some(v) = value.as_bool() { s.voice_input_enabled = v },
        "voice_input_device" => if let Some(v) = value.as_str() { s.voice_input_device = v.to_string() },
        "hotkey_play_last" => if let Some(v) = value.as_str() { s.hotkey_play_last = v.to_string() },
        "hotkey_mic_toggle" => if let Some(v) = value.as_str() { s.hotkey_mic_toggle = v.to_string() },
        "plugin_config" => if let Ok(map) = serde_json::from_value::<std::collections::HashMap<String, std::collections::HashMap<String, String>>>(value.clone()) { s.plugin_config = map },
        "floating_ball_enabled" => if let Some(v) = value.as_bool() { s.floating_ball_enabled = v },
        "floating_ball_x" => if let Some(v) = value.as_i64() { s.floating_ball_x = v as i32 },
        "floating_ball_y" => if let Some(v) = value.as_i64() { s.floating_ball_y = v as i32 },
        "floating_ball_size" => if let Some(v) = value.as_i64() { s.floating_ball_size = super::types::clamp_ball_size(v as i32) },
        "floating_ball_perf_mode" => if let Some(v) = value.as_str() { s.floating_ball_perf_mode = v.to_string() },
        "floating_ball_skin" => if let Some(v) = value.as_str() { s.floating_ball_skin = v.to_string() },
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
    fn plugin_config读写往返与白名单更新() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let mut s = Settings::default();
        s.plugin_config.insert(
            "foo-tts".into(),
            std::collections::HashMap::from([("api_key".to_string(), "sk-123".to_string())]),
        );
        save_settings(&d, &s).unwrap();
        let got = load_settings(&d);
        assert_eq!(got.plugin_config["foo-tts"]["api_key"], "sk-123");

        // update_setting 白名单
        let back = update_setting(
            &d,
            "plugin_config",
            serde_json::json!({ "foo-tts": { "api_key": "sk-456" } }),
        )
        .unwrap();
        assert_eq!(back.plugin_config["foo-tts"]["api_key"], "sk-456");
        // 其它键不丢
        assert_eq!(back.hotkey_show_window, s.hotkey_show_window);
    }

    #[test]
    fn 旧minimax键一次性迁移进plugin_config() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        // 模拟存量用户的 settings.json：旧顶层键有值
        std::fs::write(
            d.join("settings.json"),
            r#"{"minimax_api_key":"k1","minimax_global_api_key":"k2","theme":"dark"}"#,
        )
        .unwrap();
        let s = load_settings(&d);
        assert_eq!(s.plugin_config["minimax-tts"]["api_key"], "k1", "国内版 Key 应搬入");
        assert_eq!(s.plugin_config["minimax-tts-global"]["api_key"], "k2", "国际版 Key 应搬入");
        assert!(s.minimax_api_key.is_empty(), "旧键应清空");
        assert!(s.minimax_global_api_key.is_empty());
        assert_eq!(s.theme, "dark", "无关字段不受影响");

        // 迁移已落盘：重新加载不再变动（幂等）
        let raw = std::fs::read_to_string(d.join("settings.json")).unwrap();
        assert!(raw.contains("plugin_config"), "迁移结果应已写回文件");
        let s2 = load_settings(&d);
        assert_eq!(s2.plugin_config, s.plugin_config);

        // 新面板已填值时旧键不覆盖（旧值作废丢弃）
        std::fs::write(
            d.join("settings.json"),
            r#"{"minimax_api_key":"old","plugin_config":{"minimax-tts":{"api_key":"new"}}}"#,
        )
        .unwrap();
        let s3 = load_settings(&d);
        assert_eq!(s3.plugin_config["minimax-tts"]["api_key"], "new");
    }

    #[test]
    fn 未知的键忽略不报错() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let _ = update_setting(&d, "不存在的键", serde_json::json!("x")).unwrap();
    }
}