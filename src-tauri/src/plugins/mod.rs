// 插件系统：把 .dll 动态库插件加载为可用的 TTS 引擎。
//
// 组成：
// - manifest：插件清单（manifest.json）结构与校验
// - registry：已安装插件注册表（registry.json）
// - loader：libloading 加载 dll + SHA-256 校验 + TTSEngine 包装
// - manager：PluginManager（Tauri State），启动时加载全部插件
//
// 总体设计见 doc/插件系统规划.md，本阶段实现见 doc/开发记录.md 阶段 16。

pub mod loader;
pub mod manager;
pub mod manifest;
pub mod registry;

pub use loader::{LoadedPlugin, PluginEngine};
pub use manager::{PluginInfo, PluginManager};
pub use manifest::PluginManifest;

use std::path::Path;
use thiserror::Error;

use crate::storage::types::Settings;

/// 兼容迁移：内置 edge 引擎自插件系统起抽出为 "edge-tts" 插件。
/// 老设置 tts_engine == "edge" 自动改为插件 id，并把已选音色带到
/// plugin_voices["edge-tts"]，用户无感。幂等，每次启动执行。
pub fn migrate_legacy_engine(data_dir: &Path, mut settings: Settings) -> Settings {
    if settings.tts_engine == "edge" {
        let voice = std::mem::take(&mut settings.edge_voice);
        settings
            .plugin_voices
            .entry("edge-tts".to_string())
            .or_insert(voice);
        settings.tts_engine = "edge-tts".to_string();
        if let Err(e) = crate::storage::settings::save_settings(data_dir, &settings) {
            eprintln!("edge→插件迁移设置保存失败: {e}");
        }
    }
    settings
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

    #[test]
    fn 迁移_edge引擎改插件id并带音色() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.tts_engine = "edge".into();
        s.edge_voice = "zh-CN-YunyangNeural".into();

        let migrated = migrate_legacy_engine(dir.path(), s);
        assert_eq!(migrated.tts_engine, "edge-tts");
        assert_eq!(migrated.plugin_voices.get("edge-tts").map(String::as_str), Some("zh-CN-YunyangNeural"));
        // 已落盘（重启后不再触发迁移也不会丢）
        let saved = crate::storage::settings::load_settings(dir.path());
        assert_eq!(saved.tts_engine, "edge-tts");
    }

    #[test]
    fn 迁移_已有插件音色则不覆盖() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.tts_engine = "edge".into();
        s.edge_voice = "zh-CN-YunyangNeural".into();
        s.plugin_voices.insert("edge-tts".into(), "zh-CN-XiaoxiaoNeural".into());

        let migrated = migrate_legacy_engine(dir.path(), s);
        assert_eq!(migrated.plugin_voices.get("edge-tts").map(String::as_str), Some("zh-CN-XiaoxiaoNeural"));
    }

    #[test]
    fn 迁移_非edge引擎不动() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.tts_engine = "mimo".into();
        let migrated = migrate_legacy_engine(dir.path(), s);
        assert_eq!(migrated.tts_engine, "mimo");
    }
}
