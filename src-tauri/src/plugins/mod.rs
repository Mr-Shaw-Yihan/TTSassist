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
pub fn migrate_legacy_engine(config_dir: &Path) {
    let path = config_dir.join("settings.json");
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

    // 3) 内置 moss → moss-tts 插件：tts_engine 改指；非空 moss_api_key→plugin_config["moss-tts"].api_key、
    //    非空 moss_voice_id→plugin_voices["moss-tts"]（已有不覆盖、空则不写、无默认音色——MOSS 音色为自由串）。
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("moss") {
        obj.insert("tts_engine".into(), serde_json::Value::String("moss-tts".into()));
        let key = obj.get("moss_api_key").and_then(|x| x.as_str()).map(|s| s.to_string());
        obj.remove("moss_api_key");
        if let Some(k) = key {
            if !k.is_empty() {
                set_plugin_config_if_absent(obj, "moss-tts", "api_key", &k);
            }
        }
        let voice = obj.get("moss_voice_id").and_then(|x| x.as_str()).map(|s| s.to_string());
        obj.remove("moss_voice_id");
        if let Some(vd) = voice {
            if !vd.is_empty() {
                set_plugin_voice_if_absent(obj, "moss-tts", vd);
            }
        }
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

/// 写入 `plugin_config[plugin_id][key]`：已有值则保留（不覆盖用户在新面板里的填写）。
/// 用于把内置引擎遗留在 settings 根部的配置（如 MOSS 的 api_key）搬进通用插件配置。
fn set_plugin_config_if_absent(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    plugin_id: &str,
    key: &str,
    value: &str,
) {
    let pc = obj
        .entry("plugin_config")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = pc.as_object_mut() {
        let entry = map
            .entry(plugin_id.to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(em) = entry.as_object_mut() {
            em.entry(key.to_string())
                .or_insert_with(|| serde_json::Value::String(value.to_string()));
        }
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

/// 已下架、由新插件承接的遗留插件映射表（引擎无关：只认插件 id 与承接者 id，不认识任何引擎内部）。
///
/// 为什么要这张表：内置插件一旦从某个版本里移除，`bootstrap_bundled_zips` 只负责“装”不负责“删”，
/// 老用户本地 `<plugins>/<旧id>` 目录与 registry 条目会原样留着、下次启动继续加载，于是与承接者并存
/// （表现就是“两个 MiniMax 国内版”）。启动加载前按这张表一次性清走旧插件，并把手动选过旧引擎的
/// 用户改到承接者、把旧插件的 Key/音色搬到承接者（承接者已有值则作废，绝不覆盖用户在新面板填的）。
const REPLACED_PLUGINS: &[(&str, &str)] = &[
    ("minimax-tts", "minimax-tts-stream"),
];

/// 启动期清理已下架插件（须在 PluginManager::load_all 之前调用）。幂等：旧插件不在场时什么都不做。
pub fn sweep_replaced_plugins(plugins_root: &Path, config_dir: &Path) {
    for &(old, new) in REPLACED_PLUGINS {
        let mut touched = false;

        // 1) 删遗留插件代码目录（含其 pending）。不存在就是没什么可删。
        let dir = plugins_root.join(old);
        if dir.exists() {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => touched = true,
                Err(e) => log_warn!("[插件清理] 删除已下架插件目录失败 {old}：{e}"),
            }
        }

        // 2) 注册表移除旧条目（否则 load_all 仍会去加载那个目录已被删的插件）
        let mut reg = registry::load_registry(plugins_root);
        let before = reg.plugins.len();
        reg.plugins.retain(|e| e.id != old);
        if reg.plugins.len() != before {
            match registry::save_registry(plugins_root, &reg) {
                Ok(()) => touched = true,
                Err(e) => log_warn!("[插件清理] 移除注册表旧条目失败 {old}：{e}"),
            }
        }

        // 3) settings.json：引擎改指承接者 + 把旧插件的 plugin_config / plugin_voices 搬过去
        if rewrite_settings_for_replaced(config_dir, old, new) {
            touched = true;
        }

        if touched {
            log_info!("[插件清理] 已下架插件 {old} 已清理并迁移到 {new}");
        }
    }
}

/// 就 settings.json 把一个已下架引擎的选中项与配置搬到承接者。直接改原始 JSON
/// （与 migrate_legacy_engine 同法，不经 Settings 结构体，避免读写往返丢未知键）。返回是否改动。
fn rewrite_settings_for_replaced(config_dir: &Path, old: &str, new: &str) -> bool {
    let path = config_dir.join("settings.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };
    let mut value: serde_json::Value = match serde_json::from_str(raw.trim_start_matches('\u{FEFF}')) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return false,
    };
    let mut changed = false;

    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some(old) {
        obj.insert("tts_engine".into(), serde_json::Value::String(new.into()));
        changed = true;
    }
    changed |= move_subtable(obj, "plugin_config", old, new);
    changed |= move_subtable(obj, "plugin_voices", old, new);

    if changed {
        if let Err(e) = crate::storage::atomic::write_json_pretty(&path, &value) {
            log_warn!("已下架插件设置迁移落盘失败：{e}");
        }
    }
    changed
}

/// 把 `obj[field][old]` 搬到 `obj[field][new]`（new 缺失时），并删掉 `obj[field][old]`。返回是否改动。
/// new 已有值时旧值作废——与“绝不覆盖用户在新面板填的东西”是同一条红线。
fn move_subtable(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    old: &str,
    new: &str,
) -> bool {
    let Some(sub) = obj.get_mut(field).and_then(|v| v.as_object_mut()) else {
        return false;
    };
    if sub.get(old).is_none() {
        return false;
    }
    if sub.get(new).is_none() {
        if let Some(val) = sub.remove(old) {
            sub.insert(new.to_string(), val);
        }
    } else {
        sub.remove(old);
    }
    true
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
    fn 迁移_moss引擎改插件id并搬key音色() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"K1","moss_voice_id":"V1"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "moss-tts");
        assert_eq!(v["plugin_config"]["moss-tts"]["api_key"], "K1", "旧 moss_api_key 应搬进插件配置");
        assert_eq!(v["plugin_voices"]["moss-tts"], "V1", "旧 moss_voice_id 应搬进插件音色表");
        assert!(v.get("moss_api_key").is_none(), "旧字段应被清除");
        assert!(v.get("moss_voice_id").is_none(), "旧字段应被清除");
    }

    #[test]
    fn 迁移_moss已有插件配置则不覆盖() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"K1","moss_voice_id":"V1","plugin_config":{"moss-tts":{"api_key":"KNEW"}},"plugin_voices":{"moss-tts":"VNEW"}}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_config"]["moss-tts"]["api_key"], "KNEW", "用户新填的插件配置不被旧字段覆盖");
        assert_eq!(v["plugin_voices"]["moss-tts"], "VNEW", "用户新选的插件音色不被旧字段覆盖");
    }

    #[test]
    fn 迁移_moss空key音色不写入() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"","moss_voice_id":""}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "moss-tts", "引擎仍应改指");
        assert!(v["plugin_config"]["moss-tts"]["api_key"].is_null(), "空 key 不写入（避免空串占位）");
        assert!(v["plugin_voices"]["moss-tts"].is_null(), "空音色不写入");
    }

    #[test]
    fn 迁移_无设置文件不报错() {
        let dir = tempfile::tempdir().unwrap();
        migrate_legacy_engine(dir.path()); // 不 panic 即通过
    }

    #[test]
    fn 清理已下架插件并改写引擎到承接者() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let config = root.path().join("config");
        std::fs::create_dir_all(plugins.join("minimax-tts")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            plugins.join("registry.json"),
            r#"{"plugins":[{"id":"minimax-tts","version":"0.1.0","installed_at":"x"},{"id":"zeta-tts","version":"0.1.1","installed_at":"y"}],"suppressed":[]}"#,
        )
        .unwrap();
        std::fs::write(
            config.join("settings.json"),
            r#"{"tts_engine":"minimax-tts","plugin_config":{"minimax-tts":{"api_key":"K"}},"plugin_voices":{"minimax-tts":"中文女声"}}"#,
        )
        .unwrap();

        sweep_replaced_plugins(&plugins, &config);

        assert!(!plugins.join("minimax-tts").exists(), "旧插件目录应删除");
        let reg_raw = std::fs::read_to_string(plugins.join("registry.json")).unwrap();
        assert!(!reg_raw.contains("minimax-tts"), "注册表不应再有旧条目");
        assert!(reg_raw.contains("zeta-tts"), "无关插件不能被误删");

        let v = read_json(&config);
        assert_eq!(v["tts_engine"], "minimax-tts-stream");
        assert_eq!(v["plugin_config"]["minimax-tts-stream"]["api_key"], "K");
        assert_eq!(v["plugin_voices"]["minimax-tts-stream"], "中文女声");
        assert!(v["plugin_config"].get("minimax-tts").is_none(), "旧 plugin_config 应移除");
    }

    #[test]
    fn 清理已下架插件幂等且不误伤在用的() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let config = root.path().join("config");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            plugins.join("registry.json"),
            r#"{"plugins":[{"id":"zeta-tts","version":"0.1.1","installed_at":"y"}],"suppressed":[]}"#,
        )
        .unwrap();
        std::fs::write(
            config.join("settings.json"),
            r#"{"tts_engine":"zeta-tts","plugin_config":{"zeta-tts":{"api_key":"M"}}}"#,
        )
        .unwrap();

        sweep_replaced_plugins(&plugins, &config);
        let after1 = std::fs::read_to_string(config.join("settings.json")).unwrap();
        sweep_replaced_plugins(&plugins, &config);
        let after2 = std::fs::read_to_string(config.join("settings.json")).unwrap();
        assert_eq!(after1, after2, "无旧插件时幂等空操作");
        assert!(after2.contains("zeta-tts"));
        // 无关插件的目录/注册表条目不得被动
        let reg_raw = std::fs::read_to_string(plugins.join("registry.json")).unwrap();
        assert!(reg_raw.contains("zeta-tts"));
    }

    #[test]
    fn 清理时新位置已配值则不覆盖用户新值() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let config = root.path().join("config");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(plugins.join("registry.json"), r#"{"plugins":[],"suppressed":[]}"#).unwrap();
        std::fs::write(
            config.join("settings.json"),
            r#"{"tts_engine":"zeta-tts","plugin_config":{"minimax-tts":{"api_key":"OLD"},"minimax-tts-stream":{"api_key":"NEW"}}}"#,
        )
        .unwrap();

        sweep_replaced_plugins(&plugins, &config);

        let v = read_json(&config);
        assert_eq!(v["plugin_config"]["minimax-tts-stream"]["api_key"], "NEW", "不得覆盖用户已在新面板填的值");
        assert!(v["plugin_config"].get("minimax-tts").is_none(), "旧条目无论如何都要清走");
    }
}
