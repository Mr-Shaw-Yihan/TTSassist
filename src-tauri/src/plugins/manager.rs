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

/// 插件安装结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallOutcome {
    /// 直接安装并加载完成
    Installed,
    /// 插件运行中无法覆盖，重启后覆盖生效（pending）
    PendingRestart,
}

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
    /// 音色列表（实时查询自插件，动态音色插件可运行期增减）
    pub voices: Vec<plugin_api::VoiceItem>,
    /// 音频格式（如 mp3）
    pub audio_format: String,
    /// 引擎类别（manifest.category）："local" 本地离线 / "remote" 联网
    pub category: String,
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

        let mut reg = registry::load_registry(&manager.plugins_root);
        // 清理孤儿目录：不在注册表里的插件目录（来自"运行中卸载"的残留）
        sweep_orphan_dirs(&manager.plugins_root, &reg);

        // 应用 pending 更新：启动时 dll 未被占用，先覆盖安装再加载
        let mut pending_zips_to_clean: Vec<PathBuf> = Vec::new();
        let mut changed = false;
        for entry in reg.plugins.iter_mut() {
            if let Some(pz) = entry.pending_zip.take() {
                let zip_path = manager.plugins_root.join(&pz);
                if let Some(new_manifest) = apply_pending_zip(&manager.plugins_root, &entry.id, &pz) {
                    entry.version = new_manifest.version;
                }
                // 先不删 zip，等注册表存盘后再删（防止崩溃后 zip 丢失但注册表仍引用）
                pending_zips_to_clean.push(zip_path);
                changed = true;
            }
        }
        if changed {
            let _ = registry::save_registry(&manager.plugins_root, &reg);
            // 注册表已安全存盘，现在可以安全删除 pending zip
            for zp in pending_zips_to_clean {
                let _ = std::fs::remove_file(&zp);
            }
        }

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

    /// 插件是否已安装（注册表中有记录，含加载失败的）
    pub fn is_installed(&self, id: &str) -> bool {
        registry::load_registry(&self.plugins_root)
            .plugins
            .iter()
            .any(|e| e.id == id)
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

            // 音色表实时重查（动态音色插件运行期新增音色包后能立刻刷出来）
            let voices = loaded_plugin
                .as_ref()
                .map(|p| p.query_voices_json())
                .and_then(|json| serde_json::from_str::<Vec<plugin_api::VoiceItem>>(&json).ok())
                .unwrap_or_default();
            let audio_format = loaded_plugin
                .as_ref()
                .map(|p| p.audio_format.clone())
                .unwrap_or_default();
            let category = manifest
                .as_ref()
                .map(|m| m.category.clone())
                .unwrap_or_else(|| "remote".to_string());

            result.push(PluginInfo {
                id: entry.id.clone(),
                name,
                version,
                description,
                loaded: loaded_plugin.is_some(),
                error,
                voices,
                audio_format,
                category,
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

    /// 安装插件 zip（拖入安装与在线下载安装共用）。
    ///
    /// 插件运行中（dll 被占用）→ 记 pending，重启覆盖生效；否则直接覆盖 + 注册 + 加载。
    /// `expected_zip_checksum`：在线安装传索引中的 zip SHA-256，拖入安装传 None。
    pub fn install_zip(
        &self,
        zip_path: &Path,
        expected_zip_checksum: Option<&str>,
    ) -> Result<(InstallOutcome, PluginManifest), PluginError> {
        let staged = super::install::extract_and_verify(zip_path, expected_zip_checksum)?;
        let id = staged.manifest.id.clone();

        if self.is_loaded(&id) {
            // 插件运行中：dll 被占用无法覆盖 → pending，下次启动应用
            let pending_dir = self.plugins_root.join("pending");
            std::fs::create_dir_all(&pending_dir)
                .map_err(|e| PluginError::Io(format!("创建 pending 目录失败: {e}")))?;
            std::fs::copy(zip_path, pending_dir.join(format!("{id}.zip")))
                .map_err(|e| PluginError::Io(format!("保存更新包失败: {e}")))?;

            let mut reg = registry::load_registry(&self.plugins_root);
            let rel = format!("pending/{id}.zip");
            if let Some(e) = reg.plugins.iter_mut().find(|e| e.id == id) {
                e.pending_zip = Some(rel);
            } else {
                reg.plugins.push(registry::RegistryEntry {
                    id: id.clone(),
                    version: staged.manifest.version.clone(),
                    installed_at: crate::storage::types::now_iso(),
                    pending_zip: Some(rel),
                });
            }
            registry::save_registry(&self.plugins_root, &reg)?;
            return Ok((InstallOutcome::PendingRestart, staged.manifest));
        }

        // 直接安装：覆盖文件 + 注册 + 立即加载
        copy_staged_to(&staged, &self.plugins_root)?;
        let mut reg = registry::load_registry(&self.plugins_root);
        if let Some(e) = reg.plugins.iter_mut().find(|e| e.id == id) {
            e.version = staged.manifest.version.clone();
            e.pending_zip = None;
        } else {
            reg.plugins.push(registry::RegistryEntry {
                id: id.clone(),
                version: staged.manifest.version.clone(),
                installed_at: crate::storage::types::now_iso(),
                pending_zip: None,
            });
        }
        registry::save_registry(&self.plugins_root, &reg)?;

        self.load_one(&id);
        if self.get(&id).is_none() {
            let reason = self
                .failed
                .read()
                .ok()
                .and_then(|m| m.get(&id).cloned())
                .unwrap_or_else(|| "未知原因".to_string());
            return Err(PluginError::Unsupported(format!(
                "插件已安装但加载失败: {reason}"
            )));
        }
        Ok((InstallOutcome::Installed, staged.manifest))
    }
}

/// 启动时应用 pending 更新 zip：覆盖安装到插件目录。
/// 成功返回新版本清单（失败返回 None，保留旧版本）。
/// 注意：此函数不删除 zip 文件，由调用方在注册表存盘后统一删除。
fn apply_pending_zip(plugins_root: &Path, id: &str, pending_rel: &str) -> Option<PluginManifest> {
    let zip_path = plugins_root.join(pending_rel);
    let result = super::install::extract_and_verify(&zip_path, None)
        .and_then(|staged| copy_staged_to(&staged, plugins_root).map(|_| staged.manifest));
    match result {
        Ok(m) => {
            eprintln!("插件 [{id}] 已应用待更新版本 → v{}", m.version);
            Some(m)
        }
        Err(e) => {
            eprintln!("插件 [{id}] 应用待更新版本失败: {e}");
            None
        }
    }
}

/// 把已校验的暂存插件（manifest + dll）复制到 plugins/<id>/
fn copy_staged_to(
    staged: &super::install::StagedPlugin,
    plugins_root: &Path,
) -> Result<(), PluginError> {
    let dest = plugins_root.join(&staged.manifest.id);
    std::fs::create_dir_all(&dest)
        .map_err(|e| PluginError::Io(format!("创建插件目录失败: {e}")))?;
    std::fs::copy(
        staged.dir.path().join("manifest.json"),
        dest.join("manifest.json"),
    )
    .map_err(|e| PluginError::Io(format!("写入清单失败: {e}")))?;
    std::fs::copy(
        staged.dir.path().join(&staged.manifest.entry),
        dest.join(&staged.manifest.entry),
    )
    .map_err(|e| PluginError::Io(format!("写入动态库失败: {e}")))?;
    Ok(())
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
                pending_zip: None,
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

    /// 真机验证：加载本机已安装的 genie-tts 插件（走宿主完整加载链路），
    /// 校验元信息 / 本地类别 / 动态音色表。不触发合成（避免引导下载几百 MB）。
    /// 前置：先跑过 plugins/genie-tts/package.ps1 -Install。手动运行：
    /// cargo test -- --ignored genie插件加载与音色表
    #[test]
    #[cfg(target_os = "windows")]
    #[ignore = "需本机已安装 genie-tts 插件，手动运行"]
    fn genie插件加载与音色表() {
        let appdata = std::env::var("APPDATA").expect("APPDATA 环境变量");
        let plugin_dir = PathBuf::from(appdata).join("com.voiceassist.app/plugins/genie-tts");
        assert!(plugin_dir.exists(), "genie-tts 插件未安装: {}", plugin_dir.display());

        let plugin = crate::plugins::loader::LoadedPlugin::load(&plugin_dir, APP_VERSION)
            .expect("genie-tts 插件加载失败");
        assert_eq!(plugin.dll_id, "genie-tts");
        assert_eq!(plugin.audio_format, "wav");
        assert_eq!(plugin.manifest.category, "local", "本地插件类别应为 local");
        assert!(plugin.manifest.timeout_secs >= 600, "本地引擎超时应放宽");

        // 数据目录环境变量应在加载时注入（指向 <插件目录>/data）
        let env_key = "VA_PLUGIN_DATA_DIR_GENIE_TTS";
        let data_dir = std::env::var(env_key).expect("数据目录环境变量应已注入");
        assert!(data_dir.ends_with("data"), "数据目录应以 data 结尾: {data_dir}");

        // 动态音色表：应含预置角色（磁盘扫描 + 内置清单，不触发网络）
        let voices_json = plugin.query_voices_json();
        let voices: Vec<plugin_api::VoiceItem> =
            serde_json::from_str(&voices_json).expect("音色表应为合法 JSON");
        assert!(!voices.is_empty(), "动态音色表不应为空");
        assert!(voices.iter().any(|v| v.id == "feibi"), "应含预置角色 feibi");
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

    /// 把 test-plugin 的 dll 打包成合法的安装 zip（manifest + checksum 齐全）
    fn make_test_zip(zip_path: &Path, id: &str) {
        use crate::plugins::loader::sha256_file;
        use std::io::Write;

        let dll_src = build_test_plugin_dll();
        let checksum = sha256_file(&dll_src).unwrap();
        let manifest = serde_json::json!({
            "id": id,
            "name": "测试插件",
            "version": "0.2.0",
            "type": "tts_engine",
            "platform": ["windows"],
            "entry": "plugin.dll",
            "min_app_version": "1.0.0",
            "checksum": checksum,
            "description": "安装流程测试"
        });

        let file = std::fs::File::create(zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", opts).unwrap();
        zip.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
            .unwrap();
        zip.start_file("plugin.dll", opts).unwrap();
        zip.write_all(&std::fs::read(&dll_src).unwrap()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn install_zip_全新插件直接安装并加载() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_root = dir.path().join("plugins");
        std::fs::create_dir_all(&plugins_root).unwrap();
        let pm = PluginManager::load_all(dir.path());

        // dll 自报 id 固定为 test-plugin，清单 id 必须一致才能通过加载校验
        let zip = dir.path().join("fresh.zip");
        make_test_zip(&zip, "test-plugin");

        let (outcome, manifest) = pm.install_zip(&zip, None).expect("安装应成功");
        assert!(matches!(outcome, InstallOutcome::Installed));
        assert_eq!(manifest.id, "test-plugin");
        assert!(pm.get("test-plugin").is_some(), "装完应立即可用");

        // registry 已登记
        let reg = registry::load_registry(&plugins_root);
        assert!(reg.plugins.iter().any(|e| e.id == "test-plugin" && e.version == "0.2.0"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn install_zip_已加载插件走pending() {
        let dir = tempfile::tempdir().unwrap();
        install_test_plugin(dir.path(), None); // 先装好并加载 test-plugin

        // 阶段一：运行中安装 → pending（用作用域包裹，退出时卸载 dll）
        {
            let pm = PluginManager::load_all(dir.path());
            assert!(pm.get("test-plugin").is_some());

            let zip = dir.path().join("update.zip");
            make_test_zip(&zip, "test-plugin");
            let (outcome, _) = pm.install_zip(&zip, None).expect("安装应成功");
            assert!(matches!(outcome, InstallOutcome::PendingRestart));
            assert!(dir.path().join("plugins/pending/test-plugin.zip").exists());
        } // pm 在此 drop → dll 卸载，模拟进程退出

        // 阶段二：重新 load_all（模拟重启）→ pending 被应用，版本变 0.2.0，pending 清除
        let pm2 = PluginManager::load_all(dir.path());
        let plugin = pm2.get("test-plugin").expect("重启后应加载新版");
        assert_eq!(plugin.manifest.version, "0.2.0", "pending 更新应已应用");
        assert!(
            !dir.path().join("plugins/pending/test-plugin.zip").exists(),
            "应用后 pending zip 应删除"
        );
        let reg = registry::load_registry(&dir.path().join("plugins"));
        assert!(reg.plugins.iter().all(|e| e.pending_zip.is_none()));
    }

    #[test]
    fn install_zip_校验和错误拒绝() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_root = dir.path().join("plugins");
        std::fs::create_dir_all(&plugins_root).unwrap();
        let pm = PluginManager::load_all(dir.path());

        // 随便造个 zip，传错误的 zip checksum
        let zip = dir.path().join("bad.zip");
        std::fs::write(&zip, b"not a real zip").unwrap();
        let err = pm
            .install_zip(&zip, Some("0000000000000000000000000000000000000000000000000000000000000000"))
            .unwrap_err();
        assert!(err.to_string().contains("SHA-256"));
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
