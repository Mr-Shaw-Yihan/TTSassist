// 版本更新检查：对照 GitHub Releases 最新版本。
// 网络失败/无更新一律静默返回 null，不打扰用户。

use std::time::Duration;

const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Mr-Shaw-Yihan/TTSassist/releases/latest";

/// 更新信息（check_app_update 返回）
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    /// 新版本号（不带 v 前缀）
    pub version: String,
    /// Release 页面地址（浏览器打开下载）
    pub url: String,
    /// 更新说明（release body）
    pub notes: String,
}

/// 检查是否有新版本。无更新 / 网络失败返回 null（前端静默处理）。
#[tauri::command]
pub async fn check_app_update() -> Option<UpdateInfo> {
    let client = reqwest::Client::builder()
        .user_agent("VoiceAssist") // GitHub API 要求 UA
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let resp = client.get(RELEASES_LATEST_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let tag = body.get("tag_name")?.as_str()?;
    let latest = tag.trim_start_matches(['v', 'V']);
    let current = env!("CARGO_PKG_VERSION");

    // 当前版本 < 最新版本才有更新（相等或更高不提示）
    if !crate::plugins::manifest::version_less_than(current, latest) {
        return None;
    }
    Some(UpdateInfo {
        version: latest.to_string(),
        url: body
            .get("html_url")
            .and_then(|x| x.as_str())
            .unwrap_or("https://github.com/Mr-Shaw-Yihan/TTSassist/releases")
            .to_string(),
        notes: body.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}
