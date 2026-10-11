// 插件系统：把 .dll 动态库插件加载为可用的 TTS 引擎。
//
// 组成：
// - manifest：插件清单（manifest.json）结构与校验
// - registry：已安装插件注册表（registry.json）
// - loader：libloading 加载 dll + SHA-256 校验 + TTSEngine 包装
// - manager：PluginManager（Tauri State），启动时加载全部插件
//
// 总体设计见 doc/插件系统规划.md，本阶段实现见 doc/开发记录.md 阶段 16。

pub mod bridge;
pub mod config;
pub mod install;
pub mod loader;
pub mod manager;
pub mod manifest;
pub mod registry;

pub use bridge::HostBridge;
pub use loader::{
    LoadedAsrPlugin, LoadedFxPlugin, LoadedPlugin, LoadedServicePlugin, PluginEngine,
};
pub use manager::{InstallOutcome, PluginInfo, PluginManager};
pub use manifest::PluginManifest;

use std::path::Path;
use thiserror::Error;

/// 兼容迁移：内置引擎抽出为插件后，把老设置里的一次性改写落盘（幂等，每次启动执行）：
/// - `tts_engine == "edge"` → `"edge-tts"`，`edge_voice` → `plugin_voices["edge-tts"]`
/// - `tts_engine == "mimo"` → `"mimo-tts"`，`tts_model` → `plugin_voices["mimo-tts"]`
///
/// 直接改 settings.json 文件（不经过 Settings 结构体）：`edge_voice` 结构体里已无此字段，
/// 只能按原始 JSON 搬。旧音色字段 `tts_model` 仍是结构体字段，清掉它是为了让音色回到唯一
/// 真相 `plugin_voices`；随后读设置会用默认值补回、并随遗留 Key 迁移一起写回，但它不参与
/// 插件合成——完整启动序列的真实终态由 settings.rs 的启动序列测试钉住。
/// 注意：默认值改 `edge-tts` 只影响全新安装与缺字段的情况，**不会把老用户悄悄换引擎**。
pub fn migrate_legacy_engine(content_dir: &Path) {
    let path = content_dir.join("settings.json");
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
    let mut changed = false;

    // 1) 内置 edge → edge-tts 插件（带旧音色，缺失用默认晓晓）
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("edge") {
        let voice = obj
            .get("edge_voice")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("zh-CN-XiaoxiaoNeural")
            .to_string();
        obj.insert("tts_engine".into(), serde_json::Value::String("edge-tts".into()));
        obj.remove("edge_voice");
        set_plugin_voice_if_absent(obj, "edge-tts", voice);
        changed = true;
    }

    // 2) 内置 mimo → mimo-tts 插件（旧 tts_model 值即音色 id；空/"default" → 插件自述默认音色）
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("mimo") {
        let voice = obj
            .get("tts_model")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && *s != "default")
            .unwrap_or("mimo_default")
            .to_string();
        obj.insert("tts_engine".into(), serde_json::Value::String("mimo-tts".into()));
        obj.remove("tts_model");
        set_plugin_voice_if_absent(obj, "mimo-tts", voice);
        changed = true;
    }

    if !changed {
        return;
    }
    if let Err(e) = crate::storage::atomic::write_json_pretty(&path, &value) {
        log_warn!("引擎标识迁移设置保存失败: {e}");
    }
}

/// 写入 `plugin_voices[plugin_id]`：已有值则保留（不覆盖用户在新面板里的选择）。
fn set_plugin_voice_if_absent(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    plugin_id: &str,
    voice: String,
) {
    let pv = obj
        .entry("plugin_voices")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = pv.as_object_mut() {
        map.entry(plugin_id.to_string())
            .or_insert(serde_json::Value::String(voice));
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
    fn 迁移_无关引擎不动() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"edge-tts","edge_voice":"x"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "edge-tts", "已是插件 id 的引擎不应改动");
        assert_eq!(v["edge_voice"], "x", "非 edge 旧标识的遗留字段不应被顺手清掉");
    }

    #[test]
    fn 迁移_mimo引擎改插件id并搬音色() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":"冰糖"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "mimo-tts");
        assert_eq!(v["plugin_voices"]["mimo-tts"], "冰糖", "旧 tts_model 应搬进插件音色表");
        // 仅就本函数而言旧字段被清除；完整启动序列里它会被读设置的默认值补回（不参与合成），
        // 终态与幂等由 settings.rs「老mimo用户完整启动序列终态」钉住
        assert!(v.get("tts_model").is_none(), "旧字段应被清除");
    }

    #[test]
    fn 迁移_mimo空音色落到插件自述id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":""}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["mimo-tts"], "mimo_default", "空串同样归一到插件自述默认音色");
    }

    #[test]
    fn 迁移_mimo已有插件音色则不覆盖() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":"冰糖","plugin_voices":{"mimo-tts":"茉莉"}}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "mimo-tts");
        assert_eq!(v["plugin_voices"]["mimo-tts"], "茉莉", "用户在新面板选过的音色不被旧字段覆盖");
    }

    #[test]
    fn 迁移_mimo默认音色落到插件自述id() {
        let dir = tempfile::tempdir().unwrap();
        // 老版本 tts_model 的空串/"default" 都表示「用引擎默认音色」
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":"default"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["mimo-tts"], "mimo_default");
    }

    #[test]
    fn 迁移_无设置文件不报错() {
        let dir = tempfile::tempdir().unwrap();
        migrate_legacy_engine(dir.path()); // 不 panic 即通过
    }
}
