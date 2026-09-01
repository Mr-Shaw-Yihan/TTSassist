// 插件注册表（plugins/registry.json）：记录"装了哪些插件"。
//
// 启动时宿主按注册表逐个加载插件目录。文件缺失/损坏 → 当作空注册表，
// 不阻塞应用启动（最坏情况就是插件一个没加载）。

use std::path::Path;
use serde::{Deserialize, Serialize};
use super::PluginError;

const FILE: &str = "registry.json";

/// 注册表中的一条记录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryEntry {
    /// 插件 id（与目录名、manifest.id 一致）
    pub id: String,
    /// 安装时的插件版本
    pub version: String,
    /// 安装时间（ISO8601）
    pub installed_at: String,
    /// 待应用的更新 zip（相对 plugins/ 的路径，如 "pending/edge-tts.zip"）。
    /// 插件运行中无法覆盖 dll，安装时先记挂这里，下次启动时应用。
    #[serde(default)]
    pub pending_zip: Option<String>,
}

/// registry.json 结构
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    #[serde(default)]
    pub plugins: Vec<RegistryEntry>,
}

/// 读注册表。文件不存在/解析失败返回空注册表（容错优先）。
pub fn load_registry(plugins_root: &Path) -> Registry {
    let path = plugins_root.join(FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Registry::default(),
    };
    // PowerShell 等工具写出的 UTF-8 文件可能带 BOM，serde_json 不认，先剥掉
    let raw = raw.trim_start_matches('\u{FEFF}');
    serde_json::from_str(raw).unwrap_or_else(|e| {
        log_warn!("插件注册表损坏，按空处理: {e}");
        Registry::default()
    })
}

/// 原子写注册表
pub fn save_registry(plugins_root: &Path, registry: &Registry) -> Result<(), PluginError> {
    let path = plugins_root.join(FILE);
    crate::storage::atomic::write_json_pretty(&path, registry)
        .map_err(|e| PluginError::Io(format!("写入注册表失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn 缺失返回空注册表() {
        let dir = tempdir().unwrap();
        let r = load_registry(dir.path());
        assert!(r.plugins.is_empty());
    }

    #[test]
    fn 写入读回往返() {
        let dir = tempdir().unwrap();
        let r = Registry {
            plugins: vec![RegistryEntry {
                id: "edge-tts".into(),
                version: "1.0.0".into(),
                installed_at: "2026-08-04T10:00:00+08:00".into(),
                pending_zip: None,
            }],
        };
        save_registry(dir.path(), &r).unwrap();
        let back = load_registry(dir.path());
        assert_eq!(back, r);
    }

    #[test]
    fn 损坏返回空注册表() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{{坏的json").unwrap();
        let r = load_registry(dir.path());
        assert!(r.plugins.is_empty());
    }

    #[test]
    fn 带bom也能解析() {
        let dir = tempdir().unwrap();
        let r = Registry {
            plugins: vec![RegistryEntry {
                id: "edge-tts".into(),
                version: "1.0.0".into(),
                installed_at: "2026-08-04T10:00:00+08:00".into(),
                pending_zip: None,
            }],
        };
        let json = format!("\u{FEFF}{}", serde_json::to_string(&r).unwrap());
        std::fs::write(dir.path().join(FILE), json).unwrap();
        let back = load_registry(dir.path());
        assert_eq!(back, r);
    }
}
