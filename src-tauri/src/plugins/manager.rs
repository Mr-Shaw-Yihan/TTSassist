// 插件管理器（Tauri State）：启动时按 registry 加载全部插件，
// 成功者进 loaded（可合成），失败者进 failed（记原因，供 UI 展示）。
// 单个插件加载失败只打印日志，不影响主程序启动。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::loader::{LoadedPlugin, PluginEngine};
use super::manifest::PluginManifest;
use super::registry;
use super::PluginError;

/// 当前宿主版本号（编译期注入），用于 min_app_version 校验
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 对外展示的插件信息（list_plugins 命令的返回条目）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// 是否加载成功可用
    pub loaded: bool,
    /// 加载失败原因（loaded=false 时有值）
    pub error: Option<String>,
    /// 音色列表（解析自 dll 返回的 JSON）
    pub voices: Vec<plugin_api::VoiceItem>,
    /// 音频格式（如 mp3）
    pub audio_format: String,
}

/// 插件管理器。dll 一经加载常驻到进程退出（运行期卸载有崩溃风险，见 loader.rs 头注）。
pub struct PluginManager {
    plugins_root: PathBuf,
    loaded: RwLock<HashMap<String, Arc<LoadedPlugin>>>,
    /// id → 失败原因
    failed: RwLock<HashMap<String, String>>,
}

impl PluginManager {
    /// 启动时加载全部已安装插件（registry.json 为准）。
    pub fn load_all(data_dir: &Path) -> Self {
        let plugins_root = data_dir.join("plugins");
        // 目录不存在就建（首次启动）；建不了也不致命
        let _ = std::fs::create_dir_all(&plugins_root);

        let manager = Self {
            plugins_root,
            loaded: RwLock::new(HashMap::new()),
            failed: RwLock::new(HashMap::new()),
        };

        let reg = registry::load_registry(&manager.plugins_root);
        // 清理孤儿目录：不在注册表里的插件目录（来自"运行中卸载"的残留）
        sweep_orphan_dirs(&manager.plugins_root, &reg);
        for entry in &reg.plugins {
            manager.load_one(&entry.id);
        }
        manager
    }

    /// 插件根目录（<data_dir>/plugins）
    pub fn plugins_root(&self) -> &Path {
        &self.plugins_root
    }

    /// 插件是否已加载可用
    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded
            .read()
            .map(|map| map.contains_key(id))
            .unwrap_or(false)
    }

    /// 加载单个插件（id 即目录名）；结果记入 loaded 或 failed。
    /// pub：安装新插件后立即加载用。
    pub fn load_one(&self, id: &str) {
        let dir = self.plugins_root.join(id);
        match LoadedPlugin::load(&dir, APP_VERSION) {
            Ok(plugin) => {
                eprintln!("插件已加载: {id} v{}", plugin.manifest.version);
                if let Ok(mut map) = self.loaded.write() {
                    map.insert(id.to_string(), plugin);
                }
            }
            Err(e) => {
                eprintln!("插件加载失败 [{id}]: {e}");
                if let Ok(mut map) = self.failed.write() {
                    map.insert(id.to_string(), e.to_string());
                }
            }
        }
    }

    /// 取已加载插件（合成用）
    pub fn get(&self, id: &str) -> Option<Arc<LoadedPlugin>> {
        self.loaded.read().ok()?.get(id).cloned()
    }

    /// 把插件包装成 TTS 引擎；插件未加载返回 None
    pub fn build_engine(&self, id: &str, data_dir: &Path) -> Option<PluginEngine> {
        self.get(id).map(|p| PluginEngine::new(p, data_dir.to_path_buf()))
    }

    /// 已安装插件列表（含加载失败的），供插件管理页展示
    pub fn list(&self) -> Vec<PluginInfo> {
        let mut result = Vec::new();
        let reg = registry::load_registry(&self.plugins_root);

        for entry in &reg.plugins {
            let dir = self.plugins_root.join(&entry.id);
            // 展示信息：优先读磁盘上的 manifest（失败插件也能显示名称版本）
            let manifest = PluginManifest::load(&dir).ok();

            let loaded_plugin = self.get(&entry.id);
            let error = self
                .failed
                .read()
                .ok()
                .and_then(|map| map.get(&entry.id).cloned());

            let (name, version, description) = match &manifest {
                Some(m) => (m.name.clone(), m.version.clone(), m.description.clone()),
                None => (entry.id.clone(), entry.version.clone(), String::new()),
            };

            let voices = loaded_plugin
                .as_ref()
                .and_then(|p| serde_json::from_str::<Vec<plugin_api::VoiceItem>>(&p.voices_json).ok())
                .unwrap_or_default();
            let audio_format = loaded_plugin
                .as_ref()
                .map(|p| p.audio_format.clone())
                .unwrap_or_default();

            result.push(PluginInfo {
                id: entry.id.clone(),
                name,
                version,
                description,
                loaded: loaded_plugin.is_some(),
                error,
                voices,
                audio_format,
            });
        }
        result
    }

    /// 卸载插件：注册表移除 + 删除目录。
    /// dll 运行期不卸载（常驻约束）：已加载的插件本次会话内仍可用，重启后彻底消失。
    /// 若目录因 dll 被占用删不掉（Windows 特性），留给下次启动的孤儿清理。
    /// 返回提示文案（是否需要重启）。
    pub fn uninstall(&self, id: &str) -> Result<String, PluginError> {
        // id 合法性兜底（防路径穿越）
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(PluginError::Unsupported(format!("非法插件 id：{id}")));
        }
        let was_loaded = self.is_loaded(id);

        // 1. 注册表移除
        let mut reg = registry::load_registry(&self.plugins_root);
        reg.plugins.retain(|e| e.id != id);
        registry::save_registry(&self.plugins_root, &reg)?;

        // 2. 删除目录（dll 被占用时容忍失败，启动时孤儿清理兜底）
        let dir = self.plugins_root.join(id);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                eprintln!("插件目录删除失败（重启后自动清理）: {e}");
            }
        }

        // 3. 失败记录清理（loaded 不清：dll 常驻到进程退出）
        if let Ok(mut map) = self.failed.write() {
            map.remove(id);
        }

        Ok(if was_loaded {
            "已卸载。该插件本次会话内仍可使用，重启应用后彻底移除。".to_string()
        } else {
            "已卸载。".to_string()
        })
    }
}

/// 清理孤儿插件目录：plugins/ 下不在注册表中的目录（运行中卸载的残留）。
/// pending 目录由安装流程管理，不清理。删除失败只记日志。
fn sweep_orphan_dirs(plugins_root: &Path, reg: &registry::Registry) {
    let entries = match std::fs::read_dir(plugins_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "pending" {
            continue;
        }
        if !reg.plugins.iter().any(|e| e.id == name) {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => eprintln!("已清理孤儿插件目录: {name}"),
                Err(e) => eprintln!("孤儿插件目录清理失败 [{name}]: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 编译测试桩插件（首次较慢，产物缓存后接近瞬时），返回 dll 路径
    fn build_test_plugin_dll() -> PathBuf {
        use std::sync::OnceLock;
        static RESULT: OnceLock<PathBuf> = OnceLock::new();
        RESULT
            .get_or_init(|| {
                let src_tauri = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let manifest = src_tauri.join("../plugins/test-plugin/Cargo.toml");
                let target_dir = src_tauri.join("../plugins/target-test");
                let status = std::process::Command::new("cargo")
                    .args(["build", "--manifest-path"])
                    .arg(&manifest)
                    .arg("--target-dir")
                    .arg(&target_dir)
                    .status()
                    .expect("无法启动 cargo（编译测试插件）");
                assert!(status.success(), "编译测试插件失败");
                target_dir.join("debug/test_plugin.dll")
            })
            .clone()
    }

    /// 在临时目录布置一个可加载的 test-plugin（dll + manifest + registry）
    fn install_test_plugin(data_dir: &Path, checksum_override: Option<&str>) {
        use crate::plugins::loader::sha256_file;
        use crate::plugins::registry::{Registry, RegistryEntry};

        let dll_src = build_test_plugin_dll();
        let plugin_dir = data_dir.join("plugins/test-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::copy(&dll_src, plugin_dir.join("plugin.dll")).unwrap();

        let checksum = checksum_override
            .map(String::from)
            .unwrap_or_else(|| sha256_file(&dll_src).unwrap());
        let manifest = serde_json::json!({
            "id": "test-plugin",
            "name": "测试插件",
            "version": "0.1.0",
            "type": "tts_engine",
            "platform": ["windows"],
            "entry": "plugin.dll",
            "min_app_version": "1.0.0",
            "checksum": checksum,
            "description": "自动化测试用桩插件"
        });
        std::fs::write(
            plugin_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let reg = Registry {
            plugins: vec![RegistryEntry {
                id: "test-plugin".into(),
                version: "0.1.0".into(),
                installed_at: "2026-08-04T10:00:00+08:00".into(),
            }],
        };
        registry::save_registry(&data_dir.join("plugins"), &reg).unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn 加载测试插件并合成() {
        let dir = tempfile::tempdir().unwrap();
        install_test_plugin(dir.path(), None);

        let pm = PluginManager::load_all(dir.path());
        let plugin = pm.get("test-plugin").expect("测试插件应加载成功");

        // 元信息
        assert_eq!(plugin.dll_id, "test-plugin");
        assert_eq!(plugin.audio_format, "wav");
        assert!(plugin.voices_json.contains("voice-a"));

        // 合成成功路径
        let bytes = plugin.synthesize("你好", Some("voice-a")).unwrap();
        assert_eq!(bytes, b"FAKE_AUDIO|\xe4\xbd\xa0\xe5\xa5\xbd|voice-a");

        // 默认音色（voice 传 NULL）
        let bytes = plugin.synthesize("hi", None).unwrap();
        assert_eq!(bytes, b"FAKE_AUDIO|hi|default");

        // 插件报错路径（空文本）
        let err = plugin.synthesize("", None).unwrap_err();
        assert!(err.to_string().contains("文本不能为空"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn checksum不符拒绝加载() {
        let dir = tempfile::tempdir().unwrap();
        install_test_plugin(dir.path(), Some("0000000000000000000000000000000000000000000000000000000000000000".into()));

        let pm = PluginManager::load_all(dir.path());
        assert!(pm.get("test-plugin").is_none(), "校验和不符不得加载");

        // 失败原因可在 list 中看到
        let infos = pm.list();
        assert_eq!(infos.len(), 1);
        assert!(!infos[0].loaded);
        assert!(infos[0].error.as_deref().unwrap_or("").contains("校验"));
    }

    #[test]
    fn 空插件目录加载为空() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PluginManager::load_all(dir.path());
        assert!(pm.get("不存在").is_none());
        assert!(pm.list().is_empty());
    }

    /// 实网验证：加载本机已安装的 edge-tts 插件并真实合成。
    /// 需要联网（连微软服务）；默认 ignored，手动运行：
    /// cargo test -- --ignored edge插件实网合成
    #[test]
    #[cfg(target_os = "windows")]
    #[ignore = "需联网且本机已安装 edge-tts 插件，手动运行"]
    fn edge插件实网合成() {
        let appdata = std::env::var("APPDATA").expect("APPDATA 环境变量");
        let plugin_dir = PathBuf::from(appdata).join("com.voiceassist.app/plugins/edge-tts");
        assert!(plugin_dir.exists(), "edge-tts 插件未安装: {}", plugin_dir.display());

        let plugin = crate::plugins::loader::LoadedPlugin::load(&plugin_dir, APP_VERSION)
            .expect("edge-tts 插件加载失败");
        assert_eq!(plugin.dll_id, "edge-tts");
        assert_eq!(plugin.audio_format, "mp3");

        // 真实合成一句（用云希音色，验证 voice 参数透传）
        let bytes = plugin
            .synthesize("你好，这是电子声带的插件测试", Some("zh-CN-YunxiNeural"))
            .expect("edge-tts 合成失败（可能是网络或地区限制）");
        assert!(bytes.len() > 1000, "音频过小: {} 字节", bytes.len());
        // 检查 MP3 特征（ID3 标签或帧同步头）
        let looks_mp3 = bytes.starts_with(b"ID3")
            || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0);
        assert!(looks_mp3, "返回内容不是 MP3");
    }

    #[tokio::test]
    #[cfg(target_os = "windows")]
    async fn 插件引擎落盘音频文件() {
        use crate::tts::traits::{TTSEngine, TTSParams};

        let dir = tempfile::tempdir().unwrap();
        install_test_plugin(dir.path(), None);

        let pm = PluginManager::load_all(dir.path());
        let engine = pm.build_engine("test-plugin", dir.path()).expect("引擎构建");

        let rel = engine
            .generate(TTSParams::new("落盘测试"))
            .await
            .expect("合成应成功");

        assert!(rel.starts_with("audio/"));
        assert!(rel.ends_with(".wav"), "扩展名来自插件声明的格式: {rel}");
        let abs = dir.path().join(&rel);
        assert!(abs.exists(), "音频文件应已写入");
        let bytes = std::fs::read(&abs).unwrap();
        assert!(bytes.starts_with(b"FAKE_AUDIO|"));
    }
}
