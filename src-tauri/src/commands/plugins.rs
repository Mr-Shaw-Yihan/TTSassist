// 插件管理命令：列表 / 卸载 / 拖入安装 / 在线索引 / 下载安装。

use std::path::PathBuf;
use std::time::Duration;
use tauri::State;
use crate::plugins::{InstallOutcome, PluginInfo, PluginManager};

/// 官方插件索引地址（托管在 GitHub Releases 资产）
const PLUGIN_INDEX_URL: &str =
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/plugins-index.json";

// ── 已装插件 ─────────────────────────────────────

/// 列出已安装插件（含加载状态、失败原因、音色表）
#[tauri::command]
pub fn list_plugins(plugins: State<'_, PluginManager>) -> Vec<PluginInfo> {
    plugins.list()
}

/// 卸载插件：注册表移除 + 删目录。已加载的 dll 常驻到进程退出，
/// 返回文案告知用户是否需要重启。
#[tauri::command]
pub fn uninstall_plugin(
    id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    plugins.uninstall(&id).map_err(|e| e.to_string())
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
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("VoiceAssist")
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("初始化网络客户端失败: {e}"))
}

/// 拉取官方插件索引
async fn fetch_index_entries() -> Result<Vec<PluginIndexEntry>, String> {
    let client = http_client()?;
    let resp = client
        .get(PLUGIN_INDEX_URL)
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

    // 2. 下载 zip
    let client = http_client()?;
    let resp = client
        .get(&entry.download_url)
        .send()
        .await
        .map_err(|e| format!("下载插件失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载插件失败（HTTP {}）", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取下载内容失败: {e}"))?;

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
