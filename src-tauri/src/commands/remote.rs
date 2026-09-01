// 远程配置（remote-config.json）：邀请码等轻量运营数据的动态下发。
// 双通道：GitHub raw 主（8s 超时）→ 失败回退 Gitee dist raw；
// 本地缓存 24h，断网/双通道都失败时退回缓存或内置默认值，前端永远拿得到值。
// 文件只当纯数据用（字符串展示），不放任何可执行内容。

use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::State;

use crate::commands::AppState;

/// 主通道：GitHub raw（main 分支源文件）
const REMOTE_CONFIG_URL: &str =
    "https://raw.githubusercontent.com/Mr-Shaw-Yihan/TTSassist/main/plugins/remote-config.json";
/// 镜像通道：Gitee dist 分支 raw（国内免代理直连）
const REMOTE_CONFIG_MIRROR_URL: &str =
    "https://gitee.com/yihwan/TTSassist/raw/dist/remote-config.json";

/// 缓存有效期：24 小时
const CACHE_TTL_SECS: u64 = 24 * 3600;

/// 内置兜底邀请码（断网且无缓存时使用）。
/// ⚠ 轮换提醒：这是**离线兜底**，应与 `plugins/remote-config.json` 的在线值保持一致；
/// 换码时若不同步改这里，全新离线安装的用户会拿到旧码。改后需随下次发版才能送达。
const DEFAULT_INVITE_CODE: &str = "5P9J2B";

/// 远程配置。字段全部带默认值：线上 JSON 缺字段不报错，
/// 以后加新字段（如公告）老版本解析时自动忽略。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_invite_code")]
    pub mimo_invite_code: String,
}

fn default_invite_code() -> String {
    DEFAULT_INVITE_CODE.to_string()
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            mimo_invite_code: default_invite_code(),
        }
    }
}

/// 本地缓存文件结构（fetched_at 为 Unix 秒）
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    fetched_at: u64,
    data: RemoteConfig,
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("remote-config-cache.json")
}

fn read_cache(data_dir: &Path) -> Option<CacheFile> {
    let raw = std::fs::read_to_string(cache_path(data_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_cache(data_dir: &Path, data: &RemoteConfig) {
    let file = CacheFile {
        fetched_at: now_secs(),
        data: data.clone(),
    };
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::write(cache_path(data_dir), json);
    }
}

/// 在线拉取：主通道 → 镜像通道依次尝试；解析成功即返回
async fn fetch_online() -> Option<RemoteConfig> {
    let client = reqwest::Client::builder()
        .user_agent("VoiceAssist")
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    for url in [REMOTE_CONFIG_URL, REMOTE_CONFIG_MIRROR_URL] {
        let Ok(resp) = client.get(url).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(text) = resp.text().await else {
            continue;
        };
        if let Ok(cfg) = serde_json::from_str::<RemoteConfig>(&text) {
            return Some(cfg);
        }
    }
    None
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 取远程配置：缓存未过期直接返回（force=true 强制刷新）；
/// 过期则在线拉取并写缓存；拉取失败退回缓存，再退内置默认值。
/// 返回 Result 是 Tauri 对带引用入参（State）的 async 命令的硬性要求，实际不会失败。
#[tauri::command]
pub async fn get_remote_config(
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<RemoteConfig, String> {
    // 先 clone 再 await：Tauri 命令的 Future 要求 'static，不能持有 State 借用
    let data_dir = state.data_dir.clone();
    Ok(get_remote_config_inner(&data_dir, force.unwrap_or(false)).await)
}

/// 可测试内核：不依赖 Tauri State
async fn get_remote_config_inner(data_dir: &Path, force: bool) -> RemoteConfig {
    let cache = read_cache(data_dir);
    let fresh = cache
        .as_ref()
        .is_some_and(|c| now_secs().saturating_sub(c.fetched_at) < CACHE_TTL_SECS);
    if fresh && !force {
        return cache.expect("checked").data;
    }
    if let Some(cfg) = fetch_online().await {
        write_cache(data_dir, &cfg);
        return cfg;
    }
    // 拉取失败：有缓存用缓存（哪怕过期），否则内置默认值
    cache.map(|c| c.data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缺字段的线上json用默认值补齐() {
        let cfg: RemoteConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.mimo_invite_code, DEFAULT_INVITE_CODE);
    }

    #[test]
    fn 正常json按值解析() {
        let cfg: RemoteConfig =
            serde_json::from_str(r#"{"mimo_invite_code":"ABC123"}"#).unwrap();
        assert_eq!(cfg.mimo_invite_code, "ABC123");
    }

    #[test]
    fn 未知字段不报错() {
        let cfg: RemoteConfig =
            serde_json::from_str(r#"{"mimo_invite_code":"X","announcement":{"text":"hi"}}"#)
                .unwrap();
        assert_eq!(cfg.mimo_invite_code, "X");
    }

    #[tokio::test]
    async fn 有新鲜缓存时不发网络请求() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = RemoteConfig {
            mimo_invite_code: "CACHED".to_string(),
        };
        write_cache(tmp.path(), &cfg);
        // 缓存刚写入必然新鲜：即便网络拉不到也应返回缓存值
        let got = get_remote_config_inner(tmp.path(), false).await;
        assert_eq!(got.mimo_invite_code, "CACHED");
    }

    #[test]
    fn 缓存损坏时静默忽略() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(cache_path(tmp.path()), "not json").unwrap();
        assert!(read_cache(tmp.path()).is_none());
    }
}
