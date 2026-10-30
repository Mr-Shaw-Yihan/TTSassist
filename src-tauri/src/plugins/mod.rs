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

    // 3) 内置 moss → moss-tts 插件。三样东西都要搬：Key → plugin_config、选中音色 →
    //    plugin_voices、音色列表 → 通用「我的音色」plugin_custom_voices（name→label、
    //    voice_id→id）。MOSS 的音色从来就是用户手填的 voice_id（宿主不查服务端音色表），
    //    与 fish 的克隆 id 同构，所以直接落通用机制、不在本体留引擎专属字段。
    //    旧 JSON 键一律**保留**：Settings 是整结构序列化落盘的，删掉结构体字段会让
    //    下一次任意设置写入把老用户的 Key 从 settings.json 抹掉（字段两步走退役）。
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("moss") {
        obj.insert("tts_engine".into(), serde_json::Value::String("moss-tts".into()));

        if let Some(key) = non_empty_str(obj.get("moss_api_key")) {
            set_plugin_config_if_absent(obj, "moss-tts", "api_key", &key);
        }

        // 列表逐条宽松解析：缺 voice_id 的剔除，重复 id 只留第一条（前端用 id 当 key），
        // 名称空的用 id 兜底，免得「我的音色」里出现空白行
        let mut voices = legacy_moss_voices(obj.get("moss_voices"));

        // 选中音色：空则回落列表首项；仍为空就不建 plugin_voices 条目
        //（没配音色的老用户选了 moss-tts 由插件返回中文错误引导，比写个空值进去好）
        let selected = non_empty_str(obj.get("moss_voice_id"))
            .or_else(|| voices.first().map(|(id, _)| id.clone()));

        if let Some(sel) = &selected {
            // 不变式：选中值必须能在「我的音色」里找到，否则前端下拉显示空项
            //（同类缺陷修过 utils/selectValue.ts，见 9835075）
            if !voices.iter().any(|(id, _)| id == sel) {
                voices.push((sel.clone(), sel.clone()));
            }
            set_plugin_voice_if_absent(obj, "moss-tts", sel.clone());
        }
        if !voices.is_empty() {
            set_plugin_custom_voices_if_absent(obj, "moss-tts", &voices);
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

/// 取 JSON 字符串字段的非空值（去首尾空白；空串/缺失/非字符串一律 None）
fn non_empty_str(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 老 `moss_voices: [{name, voice_id}]` → `(id, label)` 列表：剔除无 id 的脏条目、
/// 按 id 去重（保留首条的名称）、名称缺失时用 id 兜底。
fn legacy_moss_voices(v: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let Some(items) = v.and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter_map(|item| {
            let id = non_empty_str(item.get("voice_id"))?;
            let label = non_empty_str(item.get("name")).unwrap_or_else(|| id.clone());
            seen.insert(id.clone()).then_some((id, label))
        })
        .collect()
}

/// 写入 `plugin_config[plugin_id][key]`：已有值则保留（不覆盖用户在新面板里填的 Key）。
fn set_plugin_config_if_absent(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    plugin_id: &str,
    key: &str,
    value: &str,
) {
    let pc = obj
        .entry("plugin_config")
        .or_insert_with(|| serde_json::json!({}));
    let Some(map) = pc.as_object_mut() else { return };
    let entry = map
        .entry(plugin_id.to_string())
        .or_insert_with(|| serde_json::json!({}));
    if let Some(fields) = entry.as_object_mut() {
        fields
            .entry(key.to_string())
            .or_insert_with(|| serde_json::Value::String(value.to_string()));
    }
}

/// 写入 `plugin_custom_voices[plugin_id]`（通用「我的音色」）：该插件已有条目则整体保留，
/// 不与迁移值合并——用户在新面板里存过的列表优先。
fn set_plugin_custom_voices_if_absent(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    plugin_id: &str,
    voices: &[(String, String)],
) {
    let items: Vec<serde_json::Value> = voices
        .iter()
        .map(|(id, label)| serde_json::json!({ "id": id, "label": label }))
        .collect();
    let pcv = obj
        .entry("plugin_custom_voices")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = pcv.as_object_mut() {
        map.entry(plugin_id.to_string())
            .or_insert(serde_json::Value::Array(items));
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

// baseok: 下线清理表只记已移除插件的 id（与 REPLACED_PLUGINS 同性质），不含任何引擎协议分支
const RETIRED_PLUGIN_IDS: &[&str] = &["genie-tts", "hojo-tts"];

/// 列出插件根下确实存在目录的已下线插件 id（顺序固定，便于日志与测试）。
pub fn find_retired_plugins(plugins_root: &Path) -> Vec<String> {
    RETIRED_PLUGIN_IDS
        .iter()
        .filter(|id| plugins_root.join(id).is_dir())
        .map(|id| (*id).to_string())
        .collect()
}

/// 该 id 是否已被下架或被替代（启动清理与内置引导共用一张口径，避免两处漂移）
pub fn is_retired_or_replaced(id: &str) -> bool {
    RETIRED_PLUGIN_IDS.contains(&id) || REPLACED_PLUGINS.iter().any(|(old, _)| *old == id)
}

/// 内置 zip 的候选落盘目录：Tauri 在 Windows 把 resource_dir 解析为 exe 根，
/// 而 resources 按源相对路径落盘为 <exe>/resources/plugins——两个层级都探测，
/// 存在即用（避免依赖 resource_dir 的精确语义）。bootstrap 与清理共用同一份。
pub fn bundled_zip_dirs(app: Option<&tauri::AppHandle>) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(a) = app {
        if let Ok(rd) = tauri::Manager::path(a).resource_dir() {
            dirs.push(rd.join("plugins"));
            dirs.push(rd.join("resources").join("plugins"));
        }
    }
    dirs.dedup();
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

/// 启动期清理已下架/已被替代的插件（须在 PluginManager::load_all 之前调用）。
///
/// 1) 删遗留插件代码目录（plugins/<id>）与引擎数据目录（plugin-data/<id>）；
/// 2) 删**安装目录内置资源里**属于这些 id 的残留 zip —— 1.9.0 的真实事故：
///    NSIS 覆盖安装不删新包里已不存在的文件，bootstrap 会把这些旧 zip 反复装回来，
///    表现为「清理一次 → 立刻装回 → 每启动一次就重新下载一遍模型」；
/// 3) 顺带删掉**被更高版本取代**的同 id 旧 zip（只会带来 pending 循环与磁盘浪费）；
/// 4) 清注册表里已无对应目录的条目（含已下架/被替代的）。
///
/// 删除失败只记日志、不 panic；有东西被清走时返回一次性提示（含释放空间），否则 None。幂等。
pub fn sweep_retired_plugins(
    plugins_root: &Path,
    plugin_data_root: &Path,
    bundled_dirs: &[std::path::PathBuf],
) -> Option<String> {
    // 下架 + 被替代的旧 id 用同一份口径，避免启动清理与内置引导两处漂移
    let mut legacy_ids: Vec<&str> = RETIRED_PLUGIN_IDS.to_vec();
    for (old, _) in REPLACED_PLUGINS {
        if !legacy_ids.contains(old) {
            legacy_ids.push(old);
        }
    }

    let mut removed_ids: Vec<&str> = Vec::new();
    let mut removed_zips = 0usize;
    let mut released = 0u64;

    for &id in &legacy_ids {
        let code_dir = plugins_root.join(id);
        let data_dir = plugin_data_root.join(id);
        let mut removed_any = false;
        for dir in [&code_dir, &data_dir] {
            if !dir.exists() {
                continue;
            }
            let bytes = crate::storage::paths::dir_size(dir);
            match std::fs::remove_dir_all(dir) {
                Ok(()) => {
                    released += bytes;
                    removed_any = true;
                }
                Err(e) => log_warn!("[插件清理] 删除已下线引擎目录失败 {}：{e}", dir.display()),
            }
        }
        if removed_any {
            removed_ids.push(id);
        }
    }

    // 内置资源里的残留 zip：下架/被替代的一律删；同名多版本的只留最高版本。
    // 顺带清「旧版本被新版本取代」的包：bootstrap 只装最高版，留着只会占盘。
    let mut candidates: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    for dir in bundled_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for p in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if !p.is_file() || p.extension().and_then(|s| s.to_str()) != Some("zip") {
                continue;
            }
            // 解析不了的 zip 留给 bootstrap 记日志，不在这里动
            if let Ok(m) = install::peek_zip_manifest(&p) {
                candidates.push((m.id, m.version, p));
            }
        }
    }
    let mut keep: std::collections::HashMap<String, (String, std::path::PathBuf)> =
        std::collections::HashMap::new();
    for (id, version, path) in candidates {
        if legacy_ids.contains(&id.as_str()) {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    removed_zips += 1;
                    log_info!("[插件清理] 已删除残留内置包 {}", path.display());
                }
                Err(e) => log_warn!(
                    "[插件清理] 删除残留内置包失败 {}：{e}",
                    path.display()
                ),
            }
            continue;
        }
        match keep.get(&id) {
            Some((kept_ver, _)) if !manifest::version_less_than(kept_ver, &version) => {
                match std::fs::remove_file(&path) {
                    Ok(()) => removed_zips += 1,
                    Err(e) => log_warn!(
                        "[插件清理] 删除旧版本内置包失败 {}：{e}",
                        path.display()
                    ),
                }
            }
            _ => {
                if let Some((_, old_path)) = keep.insert(id.clone(), (version, path)) {
                    match std::fs::remove_file(&old_path) {
                        Ok(()) => removed_zips += 1,
                        Err(e) => log_warn!(
                            "[插件清理] 删除旧版本内置包失败 {}：{e}",
                            old_path.display()
                        ),
                    }
                }
            }
        }
    }

    // 注册表：下架/被替代的条目 + 目录已不存在的孤儿条目
    let mut reg = registry::load_registry(plugins_root);
    let before = reg.plugins.len();
    reg.plugins
        .retain(|e| !legacy_ids.contains(&e.id.as_str()) && plugins_root.join(&e.id).is_dir());
    if reg.plugins.len() != before {
        if let Err(e) = registry::save_registry(plugins_root, &reg) {
            log_warn!("[插件清理] 清理注册表条目失败：{e}");
        }
    }

    if removed_ids.is_empty() && removed_zips == 0 {
        return None; // 早已清完（只动了注册表残留）时不打扰用户
    }
    let mut parts: Vec<String> = Vec::new();
    if !removed_ids.is_empty() {
        parts.push(format!("本地引擎 {}", removed_ids.join(" / ")));
    }
    if removed_zips > 0 {
        parts.push(format!("{removed_zips} 个残留内置包"));
    }
    let notice = format!(
        "已清理{}（这些引擎已下架或已被新版替代），释放 {:.1} MB",
        parts.join(" 与 "),
        released as f64 / 1024.0 / 1024.0
    );
    log_info!("[插件清理] {notice}");
    Some(notice)
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
    fn 迁移_moss引擎改插件id并搬key音色与音色列表() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"K1","moss_voice_id":"V1","moss_voices":[{"name":"曼波","voice_id":"V1"},{"name":"备用","voice_id":"V2"}]}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "moss-tts");
        assert_eq!(v["plugin_config"]["moss-tts"]["api_key"], "K1");
        assert_eq!(v["plugin_voices"]["moss-tts"], "V1");
        // 音色列表进通用「我的音色」：name→label、voice_id→id，顺序保持
        let cv = &v["plugin_custom_voices"]["moss-tts"];
        assert_eq!(cv.as_array().map(|a| a.len()), Some(2));
        assert_eq!(cv[0]["label"], "曼波");
        assert_eq!(cv[0]["id"], "V1");
        assert_eq!(cv[1]["id"], "V2");
        // 字段两步走退役：本版只停写，旧键保留（降级回旧版仍读得到 Key）
        assert_eq!(v["moss_api_key"], "K1");
        assert_eq!(v["moss_voice_id"], "V1");
        assert_eq!(v["moss_voices"].as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn 迁移_moss幂等且不覆盖插件侧已有值() {
        let dir = tempfile::tempdir().unwrap();
        // 用户已在新面板填过 Key、选过音色、存过「我的音色」——旧字段的值一律不得盖过去
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"OLD","moss_voice_id":"OLD-V","moss_voices":[{"name":"旧","voice_id":"OLD-V"}],"plugin_config":{"moss-tts":{"api_key":"NEW"}},"plugin_voices":{"moss-tts":"NEW-V"},"plugin_custom_voices":{"moss-tts":[{"id":"NEW-V","label":"新"}]}}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());
        let after1 = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        migrate_legacy_engine(dir.path());
        let after2 = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();

        assert_eq!(after1, after2, "第二次跑不应再改动文件");
        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "moss-tts");
        assert_eq!(v["plugin_config"]["moss-tts"]["api_key"], "NEW");
        assert_eq!(v["plugin_voices"]["moss-tts"], "NEW-V");
        assert_eq!(v["plugin_custom_voices"]["moss-tts"][0]["id"], "NEW-V");
    }

    #[test]
    fn 迁移_moss选中音色为空时回落列表首项() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_voice_id":"","moss_voices":[{"name":"曼波","voice_id":"V1"},{"name":"备用","voice_id":"V2"}]}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["moss-tts"], "V1", "空选中值应回落到列表首项");
    }

    #[test]
    fn 迁移_moss列表脏数据剔除并按id去重() {
        let dir = tempfile::tempdir().unwrap();
        // 空/缺 voice_id 的条目剔除；重复 id 只留第一条（前端「我的音色」用 id 当 React key）；
        // 空名称用 id 兜底，避免列表里出现空白行
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_voice_id":"V1","moss_voices":[{"name":"甲","voice_id":"V1"},{"name":"乙","voice_id":""},{"name":"丙"},{"name":"","voice_id":"V2"},{"name":"甲改名","voice_id":"V1"}]}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let cv = &read_json(dir.path())["plugin_custom_voices"]["moss-tts"];
        let list = cv.as_array().unwrap();
        assert_eq!(list.len(), 2, "只剩 V1 与 V2：{}", cv);
        assert_eq!(list[0]["id"], "V1");
        assert_eq!(list[0]["label"], "甲", "重复 id 保留第一条的名称");
        assert_eq!(list[1]["id"], "V2");
        assert_eq!(list[1]["label"], "V2", "空名称用 id 兜底");
    }

    #[test]
    fn 迁移_moss老设置无音色列表时把选中音色补进我的音色() {
        // 不变式：迁移后 plugin_voices 的值必定能在 plugin_custom_voices 里找到，
        // 否则前端下拉显示空项（同类缺陷修过 utils/selectValue.ts，见 9835075）
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"K1","moss_voice_id":"V9"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["moss-tts"], "V9");
        let cv = &v["plugin_custom_voices"]["moss-tts"];
        assert_eq!(cv.as_array().map(|a| a.len()), Some(1), "缺列表时也要让选中音色可见：{}", cv);
        assert_eq!(cv[0]["id"], "V9");
    }

    #[test]
    fn 迁移_moss选中音色不在列表里时补一条() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_voice_id":"V9","moss_voices":[{"name":"曼波","voice_id":"V1"}]}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let cv = &read_json(dir.path())["plugin_custom_voices"]["moss-tts"];
        let list = cv.as_array().unwrap();
        assert_eq!(list.len(), 2, "列表原有条目 + 补上的选中项：{}", cv);
        assert_eq!(list[1]["id"], "V9", "补在末尾，不打乱用户原有顺序");
    }

    #[test]
    fn 迁移_moss全空时只改引擎指向() {
        let dir = tempfile::tempdir().unwrap();
        // 没配 Key、没音色的 moss 用户：不建空壳子表，选了 moss-tts 由插件报中文错误引导
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"moss","moss_api_key":"","moss_voice_id":"","moss_voices":[]}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "moss-tts");
        assert!(v.get("plugin_config").is_none(), "空 Key 不应建 plugin_config 条目：{}", v);
        assert!(v.get("plugin_voices").is_none(), "空选中值不应建 plugin_voices 条目：{}", v);
        assert!(
            v.get("plugin_custom_voices").is_none(),
            "空列表不应建 plugin_custom_voices 条目：{}",
            v
        );
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

    #[test]
    fn 下线插件_启动时被识别并列入清理清单() {
        let root = tempfile::tempdir().unwrap();
        // baseok: 下线清理清单的测试必须点名具体引擎 id
        for id in ["genie-tts", "hojo-tts", "edge-tts"] {
            std::fs::create_dir_all(root.path().join(id)).unwrap();
        }
        let stale = find_retired_plugins(root.path());
        // baseok: 断言清理清单内容，必须点名具体引擎 id
        assert_eq!(
            stale,
            vec!["genie-tts".to_string(), "hojo-tts".to_string()],
            "只清理已下线的本地引擎，edge-tts 必须保留"
        );
    }

    #[test]
    fn 下线插件_清理目录与引擎数据并只通知一次() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let data = root.path().join("engine-data");
        // baseok: 构造下线引擎的残留插件目录与引擎数据
        std::fs::create_dir_all(plugins.join("genie-tts")).unwrap();
        std::fs::create_dir_all(data.join("genie-tts/models")).unwrap();
        std::fs::write(data.join("genie-tts/models/a.bin"), vec![0u8; 1024 * 1024]).unwrap();
        std::fs::create_dir_all(plugins.join("edge-tts")).unwrap();
        std::fs::write(
            plugins.join("registry.json"),
            r#"{"plugins":[{"id":"genie-tts","version":"0.1.0","installed_at":"x"},{"id":"edge-tts","version":"0.1.1","installed_at":"y"}],"suppressed":[]}"#,
        )
        .unwrap();

        let notice = sweep_retired_plugins(&plugins, &data, &[]).expect("应给出一次性提示");
        // baseok: 提示与断言必须点名被清理的引擎 id
        assert!(notice.contains("genie-tts"), "提示应点名被清理的引擎：{notice}");
        assert!(notice.contains("MB"), "提示应给出释放空间：{notice}");
        assert!(!plugins.join("genie-tts").exists());
        assert!(!data.join("genie-tts").exists());
        assert!(plugins.join("edge-tts").exists(), "未下线的插件不能误删");
        let reg = std::fs::read_to_string(plugins.join("registry.json")).unwrap();
        assert!(!reg.contains("genie-tts"), "注册表不应再有下线插件：{reg}");
        assert!(reg.contains("edge-tts"), "无关插件的注册表条目不能被误删");

        assert!(
            sweep_retired_plugins(&plugins, &data, &[]).is_none(),
            "再跑一次应为空操作"
        );
    }

    #[test]
    fn 清理会删下架插件的残留内置包并保留最高版本() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let data = root.path().join("pd");
        let bundled = root.path().join("bundled");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&bundled).unwrap();

        // 造最小可解析的插件包（peek_zip_manifest 只读 manifest.json，不校验 checksum）
        fn mk_zip(dir: &Path, file: &str, id: &str, ver: &str) {
            use std::io::Write;
            let f = std::fs::File::create(dir.join(file)).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file("manifest.json", opts).unwrap();
            write!(
                zw,
                r#"{{"id":"{id}","name":"{id}","version":"{ver}","type":"tts_engine","platform":["windows"],"entry":"plugin.dll","min_app_version":"1.0.0","checksum":"x"}}"#
            )
            .unwrap();
            zw.finish().unwrap();
        }
        // 用清单里的常量构造，避免在测试里硬写引擎名（基座护栏：本体不得出现引擎身份）
        let retired = RETIRED_PLUGIN_IDS[0];
        let replaced_old = REPLACED_PLUGINS[0].0;
        mk_zip(&bundled, "a.zip", retired, "0.1.0");
        mk_zip(&bundled, "b.zip", replaced_old, "0.2.1");
        mk_zip(&bundled, "c.zip", "zeta-tts", "0.1.0");
        mk_zip(&bundled, "d.zip", "zeta-tts", "0.1.2");

        let notice = sweep_retired_plugins(&plugins, &data, &[bundled.clone()])
            .expect("删了残留包也应提示");
        assert!(
            !bundled.join("a.zip").exists(),
            "已下架引擎的残留内置包必须删（否则 bootstrap 会装回来）：{notice}"
        );
        assert!(
            !bundled.join("b.zip").exists(),
            "已被替代的旧 id 内置包必须删"
        );
        assert!(
            !bundled.join("c.zip").exists(),
            "被更高版本取代的旧包应删"
        );
        assert!(
            bundled.join("d.zip").exists(),
            "最高版本的包必须保留"
        );
    }

    #[test]
    fn 下架与替代的id口径一致() {
        for id in RETIRED_PLUGIN_IDS {
            assert!(is_retired_or_replaced(id), "下架 id 必须判为 legacy：{id}");
        }
        for (old, _new) in REPLACED_PLUGINS {
            assert!(is_retired_or_replaced(old), "被替代的旧 id 也算：{old}");
        }
        assert!(!is_retired_or_replaced("zeta-tts"));
        assert!(!is_retired_or_replaced("zeta-tts-global"));
    }
}
