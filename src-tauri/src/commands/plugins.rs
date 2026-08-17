// 插件管理命令：列表 / 卸载 / 拖入安装 / 在线索引 / 下载安装 / 内置插件库 / 离线资源包导入。

use std::ffi::{c_char, CStr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager, State};
use crate::plugins::{config as plugin_config, InstallOutcome, PluginInfo, PluginManager};
use crate::commands::AppState;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 官方插件索引地址（托管在 GitHub Releases 资产）
const PLUGIN_INDEX_URL: &str =
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json";

/// 国内镜像索引地址（Gitee dist 分支 raw，GitHub 不可达时回退）
const PLUGIN_INDEX_MIRROR_URL: &str =
    "https://gitee.com/yihwan/TTSassist/raw/dist/plugins-index.json";

// ── 已装插件 ─────────────────────────────────────

/// 列出已安装插件（含加载状态、失败原因、音色表）
#[tauri::command]
pub fn list_plugins(plugins: State<'_, PluginManager>) -> Vec<PluginInfo> {
    plugins.list()
}

/// 卸载插件：注册表移除 + 删目录 + 清理 plugin_config 条目与环境变量。
/// 已加载的 dll 常驻到进程退出，返回文案告知用户是否需要重启。
#[tauri::command]
pub fn uninstall_plugin(
    id: String,
    plugins: State<'_, PluginManager>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    // 卸载前取 manifest：清环境变量需要声明的 env 名
    let manifest = plugins.manifest_of(&id);

    // 插件配置清理：删除 plugin_config[id]（避免残留密钥，重装后需重新填写）
    let had_config = {
        let mut settings = state
            .settings
            .write()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        settings.plugin_config.remove(&id).is_some()
    };
    if had_config {
        let settings = state.settings.read().map_err(|e| format!("读取设置失败: {e}"))?.clone();
        crate::storage::settings::save_settings(&state.data_dir, &settings)
            .map_err(|e| format!("保存设置失败: {e}"))?;
        if let Some(m) = &manifest {
            plugin_config::remove_manifest_envs(m);
        }
        notify_changed(&app, EVENT_SETTINGS_CHANGED);
    }

    plugins.uninstall(&id).map_err(|e| e.to_string())
}

// ── 通用插件配置（manifest 声明 → 设置页通用面板）────────────────

/// 配置字段视图：manifest 声明 + 当前值（secret 只回掩码，明文不出后端）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginConfigFieldView {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: String,
    pub description: String,
    pub placeholder: String,
    pub env: String,
    pub required: bool,
    pub options: Option<Vec<serde_json::Value>>,
    /// 当前值：secret 类型非空时返回掩码「已设置」；其余类型返回原值
    pub value: String,
}

/// get_plugin_config 返回：插件配置声明 + 当前已存值（脱敏后）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginConfigInfo {
    pub id: String,
    pub help_url: Option<String>,
    pub fields: Vec<PluginConfigFieldView>,
}

const SECRET_MASK: &str = "已设置";

/// 读取插件配置声明 + 当前值。secret 字段只回掩码（前端编辑走「留空保持不变」）。
#[tauri::command]
pub fn get_plugin_config(
    id: String,
    plugins: State<'_, PluginManager>,
    state: State<'_, AppState>,
) -> Result<PluginConfigInfo, String> {
    let manifest = plugins
        .manifest_of(&id)
        .ok_or_else(|| format!("插件「{id}」未安装"))?;
    let decl = manifest
        .config
        .as_ref()
        .ok_or_else(|| format!("插件「{}」没有声明配置项", manifest.name))?;

    let settings = state
        .settings
        .read()
        .map_err(|e| format!("读取设置失败: {e}"))?;
    let stored = settings.plugin_config.get(&id);

    let fields = decl
        .fields
        .iter()
        .map(|f| {
            let raw = stored
                .and_then(|m| m.get(&f.key))
                .map(|s| s.trim())
                .unwrap_or("");
            let value = if f.r#type == "secret" && !raw.is_empty() {
                SECRET_MASK.to_string()
            } else {
                raw.to_string()
            };
            PluginConfigFieldView {
                key: f.key.clone(),
                field_type: f.r#type.clone(),
                label: f.label.clone(),
                description: f.description.clone(),
                placeholder: f.placeholder.clone(),
                env: f.env.clone(),
                required: f.required,
                options: f.options.clone(),
                value,
            }
        })
        .collect();
    Ok(PluginConfigInfo {
        id,
        help_url: decl.help_url.clone(),
        fields,
    })
}

/// 批量写入插件配置：落盘 → 更新内存 → 按声明同步环境变量。
/// 只接受 manifest 声明过的字段；secret 留空且已有值时保持不变（前端不回显明文）。
#[tauri::command]
pub fn set_plugin_config(
    id: String,
    values: std::collections::HashMap<String, String>,
    plugins: State<'_, PluginManager>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<crate::storage::types::Settings, String> {
    let manifest = plugins
        .manifest_of(&id)
        .ok_or_else(|| format!("插件「{id}」未安装"))?;
    let decl = manifest
        .config
        .clone()
        .ok_or_else(|| format!("插件「{}」没有声明配置项", manifest.name))?;

    // 逐声明字段合并：values 优先；secret 空值不覆盖已有值
    let mut entry: std::collections::HashMap<String, String> = {
        let settings = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        settings.plugin_config.get(&id).cloned().unwrap_or_default()
    };
    for f in &decl.fields {
        let Some(v) = values.get(&f.key) else { continue };
        let v = v.trim();
        if v.is_empty() && f.r#type == "secret" {
            let existing = entry.get(&f.key).map(|s| !s.trim().is_empty()).unwrap_or(false);
            if existing {
                continue; // 留空保持不变
            }
        }
        entry.insert(f.key.clone(), v.to_string());
    }

    // 落盘 + 内存 + 环境变量 + 广播
    let settings = {
        let mut settings = state
            .settings
            .write()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        settings.plugin_config.insert(id.clone(), entry.clone());
        settings.clone()
    };
    crate::storage::settings::save_settings(&state.data_dir, &settings)
        .map_err(|e| format!("保存设置失败: {e}"))?;
    plugin_config::inject_manifest(&manifest, Some(&entry));
    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(settings)
}

/// 清空插件全部配置：删除存储条目 + 移除声明的环境变量。
#[tauri::command]
pub fn clear_plugin_config(
    id: String,
    plugins: State<'_, PluginManager>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<crate::storage::types::Settings, String> {
    let manifest = plugins.manifest_of(&id);
    let settings = {
        let mut settings = state
            .settings
            .write()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        settings.plugin_config.remove(&id);
        settings.clone()
    };
    crate::storage::settings::save_settings(&state.data_dir, &settings)
        .map_err(|e| format!("保存设置失败: {e}"))?;
    if let Some(m) = &manifest {
        plugin_config::remove_manifest_envs(m);
    }
    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(settings)
}

// ── 安装 ─────────────────────────────────────────

/// 拖入安装：本地 zip → SHA-256（dll 对照 manifest）→ 安装。
/// 来源不可信由前端提示用户确认。
#[tauri::command]
pub fn install_plugin_zip(
    path: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let zip_path = PathBuf::from(&path);
    if !zip_path.exists() {
        return Err(format!("文件不存在: {path}"));
    }
    let (outcome, manifest) = plugins
        .install_zip(&zip_path, None)
        .map_err(|e| e.to_string())?;
    Ok(match outcome {
        InstallOutcome::Installed => format!("插件「{}」安装成功，已加载。", manifest.name),
        InstallOutcome::PendingRestart => {
            format!("插件「{}」正在运行中，将在重启应用后完成更新。", manifest.name)
        }
    })
}

// ── 离线资源包导入 ─────────────────────────────

/// 导入离线资源包：用户从网盘/QQ 群下载的资源 zip（如 genie-tts 的 GenieData）
/// 解压到插件数据目录 `<plugins_root>/<id>/data/`。
///
/// zip 结构约定：包内含 `GenieData/` 目录（解压后与 data/ 下已有内容合并覆盖）。
/// 解压后校验关键文件，缺失则报明确错误。已下载的部分会被保留，
/// 与后续在线补齐/重新导入兼容（均幂等覆盖）。
#[tauri::command]
pub fn import_offline_resources(
    plugin_id: String,
    zip_path: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    import_offline_resources_inner(plugins.plugins_root(), &plugin_id, &zip_path)
}

/// 导入核心逻辑（不依赖 tauri State，便于单测）
fn import_offline_resources_inner(
    plugins_root: &Path,
    plugin_id: &str,
    zip_path: &str,
) -> Result<String, String> {
    use std::io::Read;

    // id 合法性兜底（防路径穿越，与 uninstall 一致）
    if plugin_id.is_empty()
        || !plugin_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("非法插件 id：{plugin_id}"));
    }
    let plugin_dir = plugins_root.join(plugin_id);
    if !plugin_dir.exists() {
        return Err(format!("插件「{plugin_id}」未安装，请先安装插件再导入资源包"));
    }
    let zip_path = PathBuf::from(zip_path);
    if !zip_path.exists() {
        return Err(format!("文件不存在: {}", zip_path.display()));
    }

    let file = std::fs::File::open(&zip_path).map_err(|e| format!("打开资源包失败: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("资源包不是合法的 zip 文件: {e}"))?;

    let dest = plugin_dir.join("data");
    std::fs::create_dir_all(&dest).map_err(|e| format!("创建数据目录失败: {e}"))?;

    let mut found_hubert = false;
    let mut found_speaker = false;
    let mut count = 0u32;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取资源包条目失败: {e}"))?;
        let name = entry.name().replace('\\', "/");
        // 跳过目录条目与 macOS 元数据
        if name.is_empty() || name.ends_with('/') || name.starts_with("__MACOSX") {
            continue;
        }
        // 路径安全：拒绝绝对路径与 .. 组件（防 zip slip）
        let rel = Path::new(&name);
        if rel.is_absolute() || rel.components().any(|c| c.as_os_str() == "..") {
            return Err(format!("资源包含非法路径: {name}"));
        }
        if name.starts_with("GenieData/chinese-hubert-base/") {
            found_hubert = true;
        }
        if name == "GenieData/speaker_encoder.onnx" {
            found_speaker = true;
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败: {e}"))?;
        }
        let mut buf = Vec::with_capacity(entry.size().min(64 << 20) as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("解压 {name} 失败: {e}"))?;
        std::fs::write(&out, &buf).map_err(|e| format!("写入 {name} 失败: {e}"))?;
        count += 1;
    }

    if !found_hubert || !found_speaker {
        return Err(
            "资源包结构不对：未找到 GenieData/chinese-hubert-base 与 GenieData/speaker_encoder.onnx。"
                .to_string()
                + "请确认下载的是「Genie 语音资源包」（zip 内应直接含 GenieData 目录）",
        );
    }

    Ok(format!(
        "资源导入成功（{count} 个文件）。现在可以点击「安装运行环境」继续完成剩余步骤"
    ))
}

/// 清除失败资源：删掉下载失败/不完整的语音资源（GenieData，含 HF 缓存元数据）
/// 与下载中转目录，让用户以干净状态重试在线下载或导入离线资源包。
/// 不动 python/（运行环境）与 characters/（已装音色）。
#[tauri::command]
pub fn clean_failed_resources(
    plugin_id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    clean_failed_resources_inner(plugins.plugins_root(), &plugin_id)
}

/// 清除核心逻辑（不依赖 tauri State，便于单测）
fn clean_failed_resources_inner(plugins_root: &Path, plugin_id: &str) -> Result<String, String> {
    // id 合法性兜底（与 import_offline_resources_inner 一致）
    if plugin_id.is_empty()
        || !plugin_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("非法插件 id：{plugin_id}"));
    }
    let data_dir = plugins_root.join(plugin_id).join("data");
    if !data_dir.exists() {
        return Ok("没有需要清除的失败资源".to_string());
    }
    let mut removed = Vec::new();
    for name in ["GenieData", ".dl", ".dl-cache"] {
        let p = data_dir.join(name);
        if p.exists() {
            std::fs::remove_dir_all(&p)
                .map_err(|e| format!("清除 {name} 失败（可能有进程占用，请关闭应用后重试）: {e}"))?;
            removed.push(name);
        }
    }
    if removed.is_empty() {
        Ok("没有需要清除的失败资源".to_string())
    } else {
        Ok(format!(
            "已清除失败的语音资源。现在可以重试在线下载，或导入离线资源包"
        ))
    }
}

// ── 在线插件索引 ─────────────────────────────────

/// 插件索引条目（官方索引 JSON 的 plugins 数组项）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginIndexEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub download_url: String,
    /// zip 包的 SHA-256（十六进制）
    pub checksum: String,
    #[serde(default)]
    pub description: String,
    /// 资源需求说明（可选，供用户在线安装前判断配置）
    #[serde(default)]
    pub requirements: Option<String>,
    /// 插件类型（tts_engine / asr_engine）；旧索引无此字段时默认 tts_engine
    #[serde(default = "default_plugin_type")]
    pub plugin_type: String,
    /// 国内镜像下载地址（Gitee dist 分支 raw，可选）；主地址不可达时回退
    #[serde(default)]
    pub mirror_url: Option<String>,
}

fn default_plugin_type() -> String {
    "tts_engine".to_string()
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("VoiceAssist")
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("初始化网络客户端失败: {e}"))
}

/// 拉取官方插件索引：GitHub 主通道（短超时）失败后回退 Gitee 镜像。
/// 索引本身很小，10 秒超时足够；失败信息只保留最后一个通道的。
async fn fetch_index_entries() -> Result<Vec<PluginIndexEntry>, String> {
    let mut last_err = String::from("未配置索引地址");
    for url in [PLUGIN_INDEX_URL, PLUGIN_INDEX_MIRROR_URL] {
        match fetch_index_from(url).await {
            Ok(entries) => return Ok(entries),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 从单个地址拉索引（10 秒短超时，保证双通道回退不拖沓）
async fn fetch_index_from(url: &str) -> Result<Vec<PluginIndexEntry>, String> {
    let client = http_client()?;
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("拉取插件索引失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("拉取插件索引失败（HTTP {}）", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("插件索引不是合法 JSON: {e}"))?;
    serde_json::from_value(
        body.get("plugins").cloned().unwrap_or(serde_json::json!([])),
    )
    .map_err(|e| format!("插件索引格式错误: {e}"))
}

/// 拉取官方插件索引（供插件页展示可安装插件）
#[tauri::command]
pub async fn fetch_plugin_index() -> Result<Vec<PluginIndexEntry>, String> {
    fetch_index_entries().await
}

/// 在线安装：索引找条目 → 下载 zip → SHA-256 对照索引 checksum → 安装
#[tauri::command]
pub async fn download_install_plugin(
    id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    // 1. 索引找条目
    let entries = fetch_index_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("索引中不存在插件「{id}」"))?;

    // 2. 下载 zip（主地址 → mirror_url 镜像依次尝试，SHA-256 双通道一致）
    let mut urls: Vec<String> = vec![entry.download_url.clone()];
    if let Some(m) = &entry.mirror_url {
        urls.push(m.clone());
    }
    let client = http_client()?;
    let bytes = download_zip_bytes(&client, &urls).await?;

    // 3. 写临时文件 → 安装（zip SHA-256 对照索引）
    let tmp = std::env::temp_dir().join(format!("va-plugin-{id}.zip"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
    let result = plugins.install_zip(&tmp, Some(&entry.checksum));
    let _ = std::fs::remove_file(&tmp);

    let (outcome, manifest) = result.map_err(|e| e.to_string())?;
    Ok(match outcome {
        InstallOutcome::Installed => format!("插件「{}」安装成功，已加载。", manifest.name),
        InstallOutcome::PendingRestart => {
            format!("插件「{}」正在运行中，将在重启应用后完成更新。", manifest.name)
        }
    })
}

/// 下载 zip 字节：按顺序尝试 urls（主地址 → 镜像），全部失败报最后一个错误。
/// 单地址 120 秒超时：zip 只有几 MB，超时即视为通道不可用，尽快切镜像。
async fn download_zip_bytes(client: &reqwest::Client, urls: &[String]) -> Result<Vec<u8>, String> {
    let mut last_err = String::from("没有可用的下载地址");
    for url in urls {
        let result = async {
            let resp = client
                .get(url)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .map_err(|e| format!("下载插件失败: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("下载插件失败（HTTP {}）", resp.status()));
            }
            resp.bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| format!("读取下载内容失败: {e}"))
        }
        .await;
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

// ── 内置插件库（随安装包分发的 zip）─────────────────

/// 内置插件条目（读自安装包资源里的 zip 内嵌 manifest）
#[derive(Debug, Clone, serde::Serialize)]
pub struct BundledPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// 资源需求说明（供用户安装前判断配置；可为空）
    pub requirements: Option<String>,
    /// 插件类型（manifest.type）：tts_engine / asr_engine，前端按此分类展示
    pub plugin_type: String,
    /// 本机是否已安装（含加载失败的）
    pub installed: bool,
}

/// 安装包资源目录下的全部插件 zip（resources/plugins/*.zip）
fn bundled_zip_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let Ok(root) = app.path().resource_dir() else {
        return Vec::new();
    };
    let dir = root.join("resources").join("plugins");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
        })
        .collect()
}

/// 读 zip 内嵌的 manifest.json（不解压到磁盘）
fn read_zip_manifest(zip_path: &Path) -> Option<crate::plugins::PluginManifest> {
    use std::io::Read;
    let file = std::fs::File::open(zip_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("manifest.json").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    serde_json::from_str(raw.trim_start_matches('\u{FEFF}')).ok()
}

/// 版本号比较：按 "." 分段逐段比较，数字段按数值比，其余按字典序；
/// 缺失段视为 "0"（如 1.2 == 1.2.0）
fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<&str> = a.split('.').collect();
    let pb: Vec<&str> = b.split('.').collect();
    for i in 0..pa.len().max(pb.len()) {
        let sa = pa.get(i).copied().unwrap_or("0");
        let sb = pb.get(i).copied().unwrap_or("0");
        let ord = match (sa.parse::<u64>(), sb.parse::<u64>()) {
            (Ok(na), Ok(nb)) => na.cmp(&nb),
            _ => sa.cmp(sb),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    std::cmp::Ordering::Equal
}

/// 列出随安装包内置的插件（插件库）。
/// 同一 id 只保留最高版本：构建残留可能把新旧多份 zip 都复制进资源目录，
/// 列表只展示最新一份，避免用户看到同插件多版本。
#[tauri::command]
pub fn list_bundled_plugins(
    app: tauri::AppHandle,
    plugins: State<'_, PluginManager>,
) -> Vec<BundledPluginInfo> {
    let mut by_id: std::collections::BTreeMap<String, BundledPluginInfo> =
        std::collections::BTreeMap::new();
    for path in bundled_zip_paths(&app) {
        let Some(m) = read_zip_manifest(&path) else {
            continue;
        };
        let info = BundledPluginInfo {
            installed: plugins.is_installed(&m.id),
            id: m.id,
            name: m.name,
            version: m.version,
            description: m.description,
            requirements: m.requirements,
            plugin_type: m.plugin_type,
        };
        match by_id.get(&info.id) {
            Some(prev) if cmp_version(&info.version, &prev.version) != std::cmp::Ordering::Greater => {}
            _ => {
                by_id.insert(info.id.clone(), info);
            }
        }
    }
    by_id.into_values().collect()
}

/// 安装内置插件（dll SHA-256 对照其 manifest.checksum）。
/// 同 id 存在多份 zip 时安装最高版本。
#[tauri::command]
pub fn install_bundled_plugin(
    app: tauri::AppHandle,
    id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let zip = bundled_zip_paths(&app)
        .into_iter()
        .filter_map(|p| {
            let m = read_zip_manifest(&p)?;
            (m.id == id).then_some((p, m.version))
        })
        .max_by(|a, b| cmp_version(&a.1, &b.1))
        .map(|(p, _)| p)
        .ok_or_else(|| format!("安装包内不存在插件「{id}」"))?;

    let (outcome, manifest) = plugins.install_zip(&zip, None).map_err(|e| e.to_string())?;
    Ok(match outcome {
        InstallOutcome::Installed => format!("插件「{}」安装成功，已加载。", manifest.name),
        InstallOutcome::PendingRestart => {
            format!("插件「{}」正在运行中，将在重启应用后完成更新。", manifest.name)
        }
    })
}

// ── 环境安装与音色管理（本地引擎 setup / voice ops）──────────────────────
//
// 阶段 21 起，引擎环境安装与音色安装共用全局单任务槽（INSTALL_BUSY）：
// 插件内部本就把这些操作串行在同一把锁上，宿主层不再制造"假并行"。
// 前端依据任务状态禁用其他安装入口，被拒只作为竞态兜底。

/// 安装进度事件名（前端 useTauriListen 监听）
pub const EVENT_PLUGIN_SETUP_PROGRESS: &str = "plugin-setup-progress";

/// 安装进度事件载荷
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupProgress {
    pub plugin_id: String,
    /// 任务类型："env" 引擎环境安装 / "voice" 音色安装
    pub kind: String,
    /// 音色 id（kind="voice" 时有值）
    pub voice_id: Option<String>,
    /// 0~100 定量进度；<0 表示不定量（以 message 为准）
    pub percent: f32,
    pub message: String,
}

/// 转发槽内容：AppHandle + 插件 id + 任务类型 + 音色 id
struct ProgressSlot {
    app: tauri::AppHandle,
    plugin_id: String,
    kind: &'static str,
    voice_id: Option<String>,
}

/// 进度转发槽：extern "C" 回调无法捕获上下文，用全局槽暂存上下文。
/// 同一时刻只允许一个安装任务（INSTALL_BUSY 保证），槽不会串。
static SETUP_SLOT: OnceLock<Mutex<Option<ProgressSlot>>> = OnceLock::new();
static INSTALL_BUSY: AtomicBool = AtomicBool::new(false);

fn setup_slot() -> &'static Mutex<Option<ProgressSlot>> {
    SETUP_SLOT.get_or_init(|| Mutex::new(None))
}

/// 插件侧进度回调：读 C 字符串 → 经全局槽转发为 Tauri 事件
unsafe extern "C" fn setup_progress_cb(percent: f32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    let msg = CStr::from_ptr(message).to_string_lossy().into_owned();
    if let Ok(guard) = setup_slot().lock() {
        if let Some(slot) = &*guard {
            let _ = slot.app.emit(
                EVENT_PLUGIN_SETUP_PROGRESS,
                SetupProgress {
                    plugin_id: slot.plugin_id.clone(),
                    kind: slot.kind.to_string(),
                    voice_id: slot.voice_id.clone(),
                    percent,
                    message: msg,
                },
            );
        }
    }
}

/// 占用全局安装任务槽；已有任务时返回 Err（前端已按状态禁用入口，此为竞态兜底）
fn acquire_install_slot(
    app: tauri::AppHandle,
    plugin_id: String,
    kind: &'static str,
    voice_id: Option<String>,
) -> Result<(), String> {
    if INSTALL_BUSY.swap(true, Ordering::SeqCst) {
        return Err("有安装任务正在进行，请等待完成后再试".into());
    }
    if let Ok(mut guard) = setup_slot().lock() {
        *guard = Some(ProgressSlot {
            app,
            plugin_id,
            kind,
            voice_id,
        });
    }
    Ok(())
}

/// 释放全局安装任务槽（与 acquire_install_slot 成对）
fn release_install_slot() {
    if let Ok(mut guard) = setup_slot().lock() {
        *guard = None;
    }
    INSTALL_BUSY.store(false, Ordering::SeqCst);
}

/// 执行插件环境安装（本地引擎下载运行环境/模型）。
/// options：JSON 字符串（可选，如 {"voice":"mika"}）。
/// 进度经 EVENT_PLUGIN_SETUP_PROGRESS 事件推送（kind="env"）；
/// 返回插件的中文结果消息。
#[tauri::command]
pub async fn run_plugin_setup(
    id: String,
    options: Option<String>,
    app: tauri::AppHandle,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let plugin = plugins
        .get(&id)
        .ok_or_else(|| format!("插件「{id}」未加载，无法执行环境安装"))?;
    if !plugin.has_setup() {
        return Err("该插件不支持环境安装".into());
    }
    acquire_install_slot(app, id.clone(), "env", None)?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        plugin.run_setup(options.as_deref(), Some(setup_progress_cb))
    })
    .await;

    release_install_slot();

    match result {
        Ok(Ok(msg)) => Ok(msg),
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(format!("安装任务中断: {e}")),
    }
}

/// 安装指定音色（预置音色首次会联网下载；环境未就绪会先补环境）。
/// 进度经 EVENT_PLUGIN_SETUP_PROGRESS 事件推送（kind="voice"）。
#[tauri::command]
pub async fn install_voice(
    id: String,
    voice_id: String,
    app: tauri::AppHandle,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let plugin = plugins
        .get(&id)
        .ok_or_else(|| format!("插件「{id}」未加载，无法安装音色"))?;
    if !plugin.has_voice_management() {
        return Err("该插件不支持音色管理".into());
    }
    acquire_install_slot(app, id.clone(), "voice", Some(voice_id.clone()))?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        plugin.install_voice(&voice_id, Some(setup_progress_cb))
    })
    .await;

    release_install_slot();

    match result {
        Ok(Ok(msg)) => Ok(msg),
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(format!("音色安装任务中断: {e}")),
    }
}

/// 卸载指定音色（删本地音色包；服务端在跑会先释放内存）。
#[tauri::command]
pub async fn uninstall_voice(
    id: String,
    voice_id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let plugin = plugins
        .get(&id)
        .ok_or_else(|| format!("插件「{id}」未加载，无法卸载音色"))?;
    if INSTALL_BUSY.load(Ordering::SeqCst) {
        return Err("有安装任务正在进行，请等待完成后再卸载".into());
    }
    tauri::async_runtime::spawn_blocking(move || plugin.uninstall_voice(&voice_id))
        .await
        .map_err(|e| format!("卸载任务中断: {e}"))?
        .map_err(|e| e.to_string())
}

/// 预加载已安装音色到内存（切换音色时调用，秒级；不触发下载）。
/// 有安装任务进行时直接跳过（返回 Ok），避免在插件锁上长时间阻塞。
#[tauri::command]
pub async fn preload_voice(
    id: String,
    voice_id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    if INSTALL_BUSY.load(Ordering::SeqCst) {
        return Ok("有安装任务正在进行，已跳过预加载".into());
    }
    let plugin = plugins
        .get(&id)
        .ok_or_else(|| format!("插件「{id}」未加载，无法预加载音色"))?;
    tauri::async_runtime::spawn_blocking(move || plugin.preload_voice(&voice_id))
        .await
        .map_err(|e| format!("预加载任务中断: {e}"))?
        .map_err(|e| e.to_string())
}

/// 导入用户自备音色包目录（插件校验布局后复制进数据目录，保留原文件）。
#[tauri::command]
pub async fn import_voice_pack(
    id: String,
    src_dir: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    let plugin = plugins
        .get(&id)
        .ok_or_else(|| format!("插件「{id}」未加载，无法导入音色"))?;
    if INSTALL_BUSY.load(Ordering::SeqCst) {
        return Err("有安装任务正在进行，请等待完成后再导入".into());
    }
    tauri::async_runtime::spawn_blocking(move || plugin.import_voice_pack(&src_dir))
        .await
        .map_err(|e| format!("导入任务中断: {e}"))?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{clean_failed_resources_inner, cmp_version, import_offline_resources_inner};
    use std::cmp::Ordering::*;

    #[test]
    fn 版本号比较() {
        assert_eq!(cmp_version("0.1.0", "1.0.0"), Less);
        assert_eq!(cmp_version("1.0.0", "0.1.0"), Greater);
        assert_eq!(cmp_version("0.1.0", "0.1.0"), Equal);
        // 缺失段视为 0
        assert_eq!(cmp_version("1.2", "1.2.0"), Equal);
        // 数值比而非字典序（10 > 9）
        assert_eq!(cmp_version("0.10.0", "0.9.0"), Greater);
    }

    /// 构造测试用资源 zip（Stored 压缩，无额外 feature 依赖）
    fn build_resource_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn 离线资源包导入成功() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("genie-tts")).unwrap();
        let zip_path = root.join("res.zip");
        build_resource_zip(
            &zip_path,
            &[
                ("GenieData/chinese-hubert-base/config.json", b"{}"),
                ("GenieData/speaker_encoder.onnx", b"onnx-bytes"),
            ],
        );
        let msg =
            import_offline_resources_inner(root, "genie-tts", zip_path.to_str().unwrap())
                .unwrap();
        assert!(msg.contains("资源导入成功"));
        assert!(root.join("genie-tts/data/GenieData/speaker_encoder.onnx").exists());
        assert!(root
            .join("genie-tts/data/GenieData/chinese-hubert-base/config.json")
            .exists());
    }

    #[test]
    fn 离线资源包结构不对被拒绝() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("genie-tts")).unwrap();
        let zip_path = root.join("bad.zip");
        build_resource_zip(&zip_path, &[("random.txt", b"hello")]);
        let err = import_offline_resources_inner(root, "genie-tts", zip_path.to_str().unwrap())
            .unwrap_err();
        assert!(err.contains("资源包结构不对"));
    }

    #[test]
    fn 离线资源包拒绝路径穿越() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("genie-tts")).unwrap();
        let zip_path = root.join("evil.zip");
        build_resource_zip(
            &zip_path,
            &[
                ("../evil.txt", b"boom"),
                ("GenieData/chinese-hubert-base/config.json", b"{}"),
                ("GenieData/speaker_encoder.onnx", b"onnx"),
            ],
        );
        let err = import_offline_resources_inner(root, "genie-tts", zip_path.to_str().unwrap())
            .unwrap_err();
        assert!(err.contains("非法路径"));
        assert!(!root.parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn 离线资源包要求插件已安装() {
        let tmp = tempfile::tempdir().unwrap();
        let err = import_offline_resources_inner(tmp.path(), "genie-tts", "whatever.zip")
            .unwrap_err();
        assert!(err.contains("未安装"));
    }

    #[test]
    fn 清除失败资源只删语音资源不动环境() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let data = root.join("genie-tts/data");
        std::fs::create_dir_all(data.join("GenieData/chinese-hubert-base")).unwrap();
        std::fs::create_dir_all(data.join(".dl")).unwrap();
        std::fs::create_dir_all(data.join("python")).unwrap();
        std::fs::create_dir_all(data.join("characters/feibi")).unwrap();

        let msg = clean_failed_resources_inner(root, "genie-tts").unwrap();
        assert!(msg.contains("已清除"));
        assert!(!data.join("GenieData").exists());
        assert!(!data.join(".dl").exists());
        // 运行环境与已装音色不受影响
        assert!(data.join("python").exists());
        assert!(data.join("characters/feibi").exists());
        // 再清一次：幂等，提示无可清除项
        let msg2 = clean_failed_resources_inner(root, "genie-tts").unwrap();
        assert!(msg2.contains("没有需要清除"));
    }
}
