// 插件系统：把 .dll 动态库插件加载为可用的 TTS 引擎。
//
// 组成：
// - manifest：插件清单（manifest.json）结构与校验
// - registry：已安装插件注册表（registry.json）
// - loader：libloading 加载 dll + SHA-256 校验 + TTSEngine 包装
// - manager：PluginManager（Tauri State），启动时加载全部插件
//
// 总体设计见 doc/插件系统规划.md，本阶段实现见 doc/开发记录.md 阶段 16。

pub mod config;
pub mod install;
pub mod loader;
pub mod manager;
pub mod manifest;
pub mod registry;

pub use loader::{LoadedAsrPlugin, LoadedPlugin, PluginEngine};
pub use manager::{InstallOutcome, PluginInfo, PluginManager};
pub use manifest::PluginManifest;

use std::path::Path;
use thiserror::Error;

/// 兼容迁移：内置 edge 引擎自插件系统起抽出为 "edge-tts" 插件。
/// 老设置 tts_engine == "edge" 自动改为插件 id，并把已选 edge_voice 带到
/// plugin_voices["edge-tts"]，用户无感。幂等，每次启动执行。
///
/// 直接改 settings.json 文件（不经过 Settings 结构体——新结构体已无 edge_voice 字段）。
pub fn migrate_legacy_engine(data_dir: &Path) {
    let path = data_dir.join("settings.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut value: serde_json::Value = match serde_json::from_str(raw.trim_start_matches('\u{FEFF}')) {
        Ok(v) => v,
        Err(_) => return,
    };
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    if obj.get("tts_engine").and_then(|x| x.as_str()) != Some("edge") {
        return;
    }
    // 带上旧音色（缺失用默认晓晓），清掉旧字段
    let voice = obj
        .get("edge_voice")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("zh-CN-XiaoxiaoNeural")
        .to_string();
    obj.insert("tts_engine".into(), serde_json::Value::String("edge-tts".into()));
    obj.remove("edge_voice");
    let pv = obj
        .entry("plugin_voices")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = pv.as_object_mut() {
        map.entry("edge-tts".to_string())
            .or_insert(serde_json::Value::String(voice));
    }
    if let Err(e) = crate::storage::atomic::write_json_pretty(&path, &value) {
        eprintln!("edge→插件迁移设置保存失败: {e}");
    }
}

/// 插件子系统的错误类型
#[derive(Debug, Error)]
pub enum PluginError {
    /// 插件目录/清单/动态库缺失
    #[error("插件文件缺失: {0}")]
    NotFound(String),

    /// manifest.json 解析失败
    #[error("清单错误: {0}")]
    Manifest(String),

    /// 平台/类型/版本/id 不满足加载条件
    #[error("插件不可用: {0}")]
    Unsupported(String),

    /// SHA-256 校验不通过（dll 可能被篡改）
    #[error("SHA-256 校验失败（期望 {expected}，实际 {actual}），dll 可能已损坏或被篡改")]
    Checksum { expected: String, actual: String },

    /// libloading 加载失败
    #[error("动态库加载失败: {0}")]
    DlOpen(String),

    /// dll 缺少约定的导出符号
    #[error("导出符号错误: {0}")]
    Symbol(String),

    /// 合成调用失败（插件返回的错误消息）
    #[error("{0}")]
    Synthesize(String),

    /// 文件 IO 错误
    #[error("IO 错误: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_json(dir: &Path) -> serde_json::Value {
        let raw = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn 迁移_edge引擎改插件id并带音色() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"edge","edge_voice":"zh-CN-YunyangNeural"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "edge-tts");
        assert_eq!(v["plugin_voices"]["edge-tts"], "zh-CN-YunyangNeural");
        assert!(v.get("edge_voice").is_none(), "旧字段应被清除");
    }

    #[test]
    fn 迁移_已有插件音色则不覆盖() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"edge","edge_voice":"zh-CN-YunyangNeural","plugin_voices":{"edge-tts":"zh-CN-XiaoxiaoNeural"}}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["edge-tts"], "zh-CN-XiaoxiaoNeural");
    }

    #[test]
    fn 迁移_非edge引擎不动() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","edge_voice":"x"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "mimo", "非 edge 引擎不应改动");
    }

    #[test]
    fn 迁移_无设置文件不报错() {
        let dir = tempfile::tempdir().unwrap();
        migrate_legacy_engine(dir.path()); // 不 panic 即通过
    }
}
