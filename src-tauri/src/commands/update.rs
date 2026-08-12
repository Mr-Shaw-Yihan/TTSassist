// 版本更新检查：对照官方发布的最新版本。
// 双通道：GitHub API（主，含完整更新说明）→ Gitee tags API（镜像，国内免代理）；
// 网络失败/无更新一律静默返回 null，不打扰用户。

use std::time::Duration;

const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Mr-Shaw-Yihan/TTSassist/releases/latest";

/// 镜像通道：Gitee 仓库 tags 列表（公开仓库匿名可读）。
/// 注意：发版时必须把版本 tag 也推到 Gitee（git push gitee <tag>），此通道才有数据。
const GITEE_TAGS_URL: &str = "https://gitee.com/api/v5/repos/yihwan/TTSassist/tags";

const RELEASES_PAGE_URL: &str = "https://github.com/Mr-Shaw-Yihan/TTSassist/releases";

/// 更新信息（check_app_update 返回）
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    /// 新版本号（不带 v 前缀）
    pub version: String,
    /// Release 页面地址（浏览器打开下载）
    pub url: String,
    /// 更新说明（release body；镜像通道拿不到时为空）
    pub notes: String,
}

/// 检查是否有新版本。无更新 / 双通道都失败返回 null（前端静默处理）。
#[tauri::command]
pub async fn check_app_update() -> Option<UpdateInfo> {
    // 主通道：GitHub API。Ok(None) = 确实无更新（不再试镜像）；Err = 网络失败（回退镜像）
    match check_via_github().await {
        Ok(info) => info,
        Err(()) => check_via_gitee().await,
    }
}

/// GitHub API 通道：带完整 release notes
async fn check_via_github() -> Result<Option<UpdateInfo>, ()> {
    let client = reqwest::Client::builder()
        .user_agent("VoiceAssist") // GitHub API 要求 UA
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| ())?;
    let resp = client.get(RELEASES_LATEST_URL).send().await.map_err(|_| ())?;
    if !resp.status().is_success() {
        return Err(());
    }
    let body: serde_json::Value = resp.json().await.map_err(|_| ())?;
    let tag = body.get("tag_name").and_then(|x| x.as_str()).ok_or(())?;
    let latest = tag.trim_start_matches(['v', 'V']);
    let current = env!("CARGO_PKG_VERSION");

    // 当前版本 < 最新版本才有更新（相等或更高不提示）
    if !crate::plugins::manifest::version_less_than(current, latest) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: latest.to_string(),
        url: body
            .get("html_url")
            .and_then(|x| x.as_str())
            .unwrap_or(RELEASES_PAGE_URL)
            .to_string(),
        notes: body.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    }))
}

/// Gitee 镜像通道：从 tags 列表里找最大的应用版本 tag（notes 拿不到，留空）
async fn check_via_gitee() -> Option<UpdateInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .ok()?;
    let resp = client.get(GITEE_TAGS_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let tags: Vec<GiteeTag> = resp.json().await.ok()?;
    let latest = pick_latest_app_version(tags.iter().map(|t| t.name.as_str()))?;
    let current = env!("CARGO_PKG_VERSION");
    if !crate::plugins::manifest::version_less_than(current, latest) {
        return None;
    }
    Some(UpdateInfo {
        version: latest.to_string(),
        url: RELEASES_PAGE_URL.to_string(),
        notes: String::new(),
    })
}

/// Gitee tags API 返回条目（只取 name，其余字段忽略）
#[derive(serde::Deserialize)]
struct GiteeTag {
    #[serde(default)]
    name: String,
}

/// 从 tag 名列表中挑出最大的应用版本号（只认 v?x.y.z 形式，
/// 过滤 plugins-v0.3.0 这类插件 tag）；返回值不带 v 前缀
fn pick_latest_app_version<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    for name in names {
        let ver = name.trim_start_matches(['v', 'V']);
        if !is_app_version(ver) {
            continue;
        }
        match best {
            None => best = Some(ver),
            Some(b) if crate::plugins::manifest::version_less_than(b, ver) => best = Some(ver),
            _ => {}
        }
    }
    best
}

/// 是否形如 x.y.z 的应用版本号（各段纯数字，至少两段）
fn is_app_version(v: &str) -> bool {
    let segs: Vec<&str> = v.split('.').collect();
    segs.len() >= 2
        && segs
            .iter()
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 过滤插件tag只认应用版本() {
        assert!(is_app_version("1.6.0"));
        assert!(is_app_version("1.6"));
        assert!(!is_app_version("plugins-v0.3.0"));
        assert!(!is_app_version("1.6.0-beta"));
        assert!(!is_app_version("1"));
        assert!(!is_app_version(""));
    }

    #[test]
    fn 从混合tag里挑最大应用版本() {
        let tags = ["plugins-v0.3.0", "v1.5.0", "v1.6.0", "1.4.0", "dist"];
        assert_eq!(pick_latest_app_version(tags.iter().copied()), Some("1.6.0"));
    }

    #[test]
    fn 没有应用版本tag时返回none() {
        let tags = ["plugins-v0.3.0", "dist"];
        assert_eq!(pick_latest_app_version(tags.iter().copied()), None);
    }
}
