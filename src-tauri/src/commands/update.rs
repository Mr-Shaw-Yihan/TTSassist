// 版本更新：检查 → 应用内下载 → 校验 → 拉起安装包升级重启。
//
// 检查优先读发布清单 app-version.json（版本/说明/校验值/双链的唯一权威源）：
// Gitee dist 分支 raw 优先（国内直达），失败再试 GitHub releases/latest 资产；
// 清单不可用时回退旧的 GitHub API / Gitee tags 通道（只有版本号与说明、没下载直链，
// 此时前端只能“前往下载页”）。网络全挂或无更新一律静默返回 null，不打扰用户。
//
// 因为最终要执行下载来的安装包：下载直链必须命中编译期白名单，落地文件必须过
// SHA-256，且安装只允许执行本模块下载并登记过的路径。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use tauri::{AppHandle, Emitter, Manager};

/// 清单主通道：Gitee dist 分支 raw（国内可达）。发版必须同步更新此文件。
const MANIFEST_GITEE_URL: &str =
    "https://gitee.com/yihwan/TTSassist/raw/dist/app-version.json";

/// 清单次通道：GitHub 本体 Release 资产（latest 恒指最新非 prerelease 本体）。
const MANIFEST_GITHUB_URL: &str =
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/app-version.json";

/// 安装包直链允许的前缀（白名单，防清单被篡改后指向任意地址执行）。
const ALLOWED_URL_PREFIXES: &[&str] = &[
    "https://gitee.com/yihwan/TTSassist/releases/download/",
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/",
];

/// 旧回退通道：GitHub API（带完整 release 说明）
const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Mr-Shaw-Yihan/TTSassist/releases/latest";

/// 旧回退通道镜像：Gitee 仓库 tags 列表（公开仓库匿名可读）。
/// 注意：发版时必须把版本 tag 也推到 Gitee（git push gitee <tag>），此通道才有数据。
const GITEE_TAGS_URL: &str = "https://gitee.com/api/v5/repos/yihwan/TTSassist/tags";

const RELEASES_PAGE_URL: &str = "https://github.com/Mr-Shaw-Yihan/TTSassist/releases";

/// 更新进度事件名（前端 useTauriListen 订阅）
pub const EVENT_UPDATE_PROGRESS: &str = "app-update-progress";

/// 单通道下载“停滞”判定：连续这么久没收到新字节即视为该通道不可用，切下一条。
const STALL_TIMEOUT: Duration = Duration::from_secs(90);

/// 发布清单 app-version.json 的结构（多余字段忽略；缺关键字段视为无效）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct AppUpdateManifest {
    pub version: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub gitee_url: String,
    #[serde(default)]
    pub github_url: String,
}

/// 更新信息（check_app_update 返回）。`has_download == false` 时下载字段无效，
/// 前端应退回“前往下载页”的旧行为。
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    /// 新版本号（不带 v 前缀）
    pub version: String,
    /// Release 页面地址（浏览器打开下载）
    pub url: String,
    /// 更新说明（markdown；旧 Gitee tags 通道拿不到时为空）
    pub notes: String,
    /// 是否具备应用内下载+安装的条件（清单通道才有）
    pub has_download: bool,
    /// 安装包文件名
    pub file: String,
    /// 字节数（0 = 未知）
    pub size: u64,
    /// 安装包 SHA-256（十六进制）
    pub sha256: String,
    /// 国内主通道
    pub gitee_url: String,
    /// 海外次通道
    pub github_url: String,
}

/// 下载进度事件载荷
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateProgress {
    /// downloading / verifying / done / failed
    pub phase: String,
    /// 0.0~1.0；总大小未知时为 -1
    pub percent: f64,
    /// 当前通道：gitee / github / ""
    pub channel: String,
    /// 面向用户的中文说明（失败原因等）
    pub message: String,
}

/// 下载完成信息；只有这里返回过的路径才允许被安装
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadedInfo {
    pub path: String,
    pub version: String,
    pub size: u64,
    pub sha256: String,
    pub channel: String,
}

/// 应用内更新运行态（Tauri manage）
pub struct AppUpdaterState {
    /// 已有下载在进行（拒并发）
    busy: AtomicBool,
    /// 取消标志（下载循环逐块检查）
    cancel: std::sync::Arc<AtomicBool>,
}

impl AppUpdaterState {
    pub fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
            cancel: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for AppUpdaterState {
    fn default() -> Self {
        Self::new()
    }
}

/// 已通过校验、允许安装的登记表（防“校验完之后文件被替换”再安装）
fn verified_registry() -> &'static Mutex<Option<DownloadedInfo>> {
    static REG: OnceLock<Mutex<Option<DownloadedInfo>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(None))
}

/// 检查是否有新版本。无更新 / 所有通道都失败返回 null（前端静默处理）。
#[tauri::command]
pub async fn check_app_update() -> Option<UpdateInfo> {
    // 首选：发布清单（带说明与双下载直链，可应用内升级）
    if let Some(m) = fetch_manifest().await {
        match manifest_to_info(&m) {
            // Ok(Some) = 有更新；Ok(None) = 确认无更新（两种情况都不再走旧通道）
            Ok(info) => return info,
            // 清单无效（字段缺/被篡改）时不退回“半截信息”，记日志后走旧通道
            Err(e) => eprintln!("[update] 清单无效，回退旧通道：{e}"),
        }
    }
    // 回退：GitHub API（有说明）→ Gitee tags（只有版本号）；均无直链，只能跳网页
    match check_via_github().await {
        Ok(info) => info,
        Err(()) => check_via_gitee().await,
    }
}

impl AppUpdateManifest {
    /// 有序的下载通道列表（国内优先）：gitee → github，空地址自动剔除。
    pub fn channels(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        if !self.gitee_url.trim().is_empty() {
            v.push(("gitee", self.gitee_url.trim().to_string()));
        }
        if !self.github_url.trim().is_empty() {
            v.push(("github", self.github_url.trim().to_string()));
        }
        v
    }

    /// 校验下载字段自洽且安全：文件名必须与版本对应、校验值形状合法、
    /// 每条直链都命中域名白名单且以该文件名结尾。
    pub fn validate_download(&self) -> Result<(), String> {
        let ver = self.version.trim();
        let expect = format!("TTSassist_{ver}_x64-setup.exe");
        if self.file != expect {
            return Err(format!("清单文件名异常：期望 {expect}，实得「{}」", self.file));
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("清单 sha256 不是 64 位十六进制".into());
        }
        let chans = self.channels();
        if chans.is_empty() {
            return Err("清单没有任何可用下载直链".into());
        }
        for (_, u) in &chans {
            if !ALLOWED_URL_PREFIXES.iter().any(|p| u.starts_with(p)) {
                return Err(format!("下载直链域名不在白名单：{u}"));
            }
            if !u.ends_with(&expect) {
                return Err(format!("下载直链与文件名不符：{u}"));
            }
        }
        Ok(())
    }
}

/// 清单 → 前端信息：无更新时 Ok(None)；清单本身不合法时 Err（调用方据此回退）。
fn manifest_to_info(m: &AppUpdateManifest) -> Result<Option<UpdateInfo>, String> {
    let version = m.version.trim().to_string();
    if !is_app_version(&version) {
        return Err(format!("清单版本号格式异常：{version}"));
    }
    let current = env!("CARGO_PKG_VERSION");
    if !crate::plugins::manifest::version_less_than(current, &version) {
        return Ok(None);
    }
    m.validate_download()?;
    Ok(Some(UpdateInfo {
        url: format!("{RELEASES_PAGE_URL}/tag/v{version}"),
        has_download: true,
        version,
        notes: m.notes.clone(),
        file: m.file.clone(),
        size: m.size,
        sha256: m.sha256.clone().to_ascii_lowercase(),
        gitee_url: m.gitee_url.trim().to_string(),
        github_url: m.github_url.trim().to_string(),
    }))
}

/// 依次从 Gitee raw、GitHub Release 资产拉清单；单通道短超时，失败即切下一条。
async fn fetch_manifest() -> Option<AppUpdateManifest> {
    let client = reqwest::Client::builder()
        .user_agent("VoiceAssist") // GitHub 侧要求 UA
        .build()
        .ok()?;
    for (url, secs) in [(MANIFEST_GITEE_URL, 8u64), (MANIFEST_GITHUB_URL, 10)] {
        match get_json::<AppUpdateManifest>(&client, url, Duration::from_secs(secs)).await {
            Ok(m) => return Some(m),
            Err(e) => eprintln!("[update] 清单通道不可用（{url}）：{e}"),
        }
    }
    None
}

/// GET 并反序列化 JSON（单请求超时；容忍 raw 文件的 UTF-8 BOM）。
async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<T, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let raw = resp.text().await.map_err(|e| e.to_string())?;
    let body = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    serde_json::from_str(body).map_err(|e| format!("清单解析失败：{e}"))
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
        // 旧通道拿不到下载直链与校验值，只能跳网页手动下载
        has_download: false,
        file: String::new(),
        size: 0,
        sha256: String::new(),
        gitee_url: String::new(),
        github_url: String::new(),
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
        has_download: false,
        file: String::new(),
        size: 0,
        sha256: String::new(),
        gitee_url: String::new(),
        github_url: String::new(),
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

// ── 应用内下载 ─────────────────────────────────────

/// 下载落地路径（%TEMP%\<安装包名>）
fn download_target(file: &str) -> PathBuf {
    std::env::temp_dir().join(file)
}

fn emit_progress(app: &AppHandle, phase: &str, percent: f64, channel: &str, message: &str) {
    let _ = app.emit(
        EVENT_UPDATE_PROGRESS,
        UpdateProgress {
            phase: phase.to_string(),
            percent,
            channel: channel.to_string(),
            message: message.to_string(),
        },
    );
}

fn fmt_mb(bytes: u64) -> String {
    if bytes == 0 {
        return "未知".into();
    }
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
}

/// 取消正在进行的下载（无下载时空操作）
#[tauri::command]
pub fn cancel_app_update(app: AppHandle) {
    let state = app.state::<AppUpdaterState>();
    state.cancel.store(true, Ordering::SeqCst);
}

/// 下载新版安装包：按 gitee → github 依次尝试，流式写盘 + 边下边算 SHA-256 + 进度事件。
/// 两条通道都失败时 Err 内是面向用户的中文原因，前端据此展示群号与手动直链兜底。
#[tauri::command]
pub async fn download_app_update(app: AppHandle) -> Result<DownloadedInfo, String> {
    {
        let state = app.state::<AppUpdaterState>();
        if state.busy.swap(true, Ordering::SeqCst) {
            return Err("已有下载在进行，请稍候".into());
        }
        state.cancel.store(false, Ordering::SeqCst);
    }
    let result = download_impl(&app).await;
    {
        let state = app.state::<AppUpdaterState>();
        state.busy.store(false, Ordering::SeqCst);
    }
    result
}

async fn download_impl(app: &AppHandle) -> Result<DownloadedInfo, String> {
    let m = fetch_manifest()
        .await
        .ok_or_else(|| "取不到更新清单，请检查网络或稍后重试".to_string())?;
    // 先校验再谈下载：清单被篡改时连请求都不该发出去
    m.validate_download()?;
    let current = env!("CARGO_PKG_VERSION");
    if !crate::plugins::manifest::version_less_than(current, m.version.trim()) {
        return Err("当前已是最新版本".into());
    }

    let cancel = {
        let state = app.state::<AppUpdaterState>();
        state.cancel.clone()
    };
    let client = reqwest::Client::builder()
        .user_agent("VoiceAssist")
        .build()
        .map_err(|e| e.to_string())?;
    let target = download_target(&m.file);

    let mut last_err = String::from("没有可用的下载通道");
    for (channel, url) in m.channels() {
        if cancel.load(Ordering::SeqCst) {
            return Err("已取消下载".into());
        }
        emit_progress(app, "downloading", 0.0, channel, &format!("正在从 {channel} 通道下载…"));
        match download_one(&client, &url, &target, &m, channel, app, &cancel).await {
            Ok(info) => {
                if let Ok(mut guard) = verified_registry().lock() {
                    *guard = Some(info.clone());
                }
                emit_progress(app, "done", 1.0, channel, "下载完成，校验通过");
                return Ok(info);
            }
            Err(e) => {
                // 半途失败或校验不过都清掉，绝不留半成品可能被安装
                let _ = std::fs::remove_file(&target);
                last_err = e;
                eprintln!("[update] 通道 {channel} 失败：{last_err}");
            }
        }
    }
    emit_progress(app, "failed", -1.0, "", &last_err);
    Err(last_err)
}

/// 单通道下载：返回前已完成大小与 SHA-256 双重校验。
async fn download_one(
    client: &reqwest::Client,
    url: &str,
    target: &PathBuf,
    m: &AppUpdateManifest,
    channel: &'static str,
    app: &AppHandle,
    cancel: &AtomicBool,
) -> Result<DownloadedInfo, String> {
    use sha2::{Digest, Sha256};

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    // 总大小优先用 HTTP 头，缺失时退到清单声明值
    let declared = if m.size > 0 { Some(m.size) } else { None };
    let total = resp.content_length().or(declared);
    let mut file = std::fs::File::create(target).map_err(|e| format!("创建临时文件失败：{e}"))?;
    let mut hasher = Sha256::new();
    let mut got: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();

    loop {
        // 停滞保护：STALL_TIMEOUT 内拿不到下一块就判该通道不可用，交给上层切下一条
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Err(_) => return Err("下载超时（通道无响应）".into()),
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(format!("下载中断：{e}")),
            Ok(Some(Ok(b))) => b,
        };
        if cancel.load(Ordering::SeqCst) {
            return Err("已取消下载".into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .map_err(|e| format!("写入失败：{e}"))?;
        got += chunk.len() as u64;

        let percent = match total {
            Some(t) if t > 0 => (got as f64 / t as f64).min(1.0),
            _ => -1.0,
        };
        // 节流：每约 512 KB 或收尾时上报一次，避免事件洪泛
        if got - last_emit >= 512_000 || total.map(|t| got >= t).unwrap_or(true) {
            last_emit = got;
            emit_progress(
                app,
                "downloading",
                percent,
                channel,
                &format!("已下载 {} / {}", fmt_mb(got), fmt_mb(total.unwrap_or(0))),
            );
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    if m.size > 0 && got != m.size {
        return Err(format!("大小不符：期望 {} 字节，实得 {got}", m.size));
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(m.sha256.trim()) {
        return Err(format!("校验不符：期望 {}，实得 {actual}", m.sha256));
    }
    Ok(DownloadedInfo {
        path: target.display().to_string(),
        version: m.version.trim().to_string(),
        size: got,
        sha256: actual,
        channel: channel.to_string(),
    })
}

// ── 安装并重启 ─────────────────────────────────────

/// 拉起安装包并退出本程序。只接受先前下载并校验通过、且此刻哈希仍一致的路径。
#[tauri::command]
pub fn install_app_update(app: AppHandle, path: String) -> Result<(), String> {
    let recorded = {
        let guard = verified_registry()
            .lock()
            .map_err(|_| "内部状态异常".to_string())?;
        guard.clone()
    };
    let recorded = recorded.ok_or_else(|| "还没有已校验的安装包，请先下载".to_string())?;
    if recorded.path != path {
        return Err("安装包路径与本次下载记录不一致，已拒绝执行".into());
    }
    let p = PathBuf::from(&path);
    let name = p
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "非法安装包路径".to_string())?;
    let expect = format!("TTSassist_{}_x64-setup.exe", recorded.version);
    if name != expect || !p.starts_with(std::env::temp_dir()) {
        return Err("安装包名称或位置异常，已拒绝执行".into());
    }
    // 再算一次哈希：防“校验完成之后文件被替换”
    let actual = crate::plugins::loader::sha256_file(&p).map_err(|e| e.to_string())?;
    if !actual.eq_ignore_ascii_case(&recorded.sha256) {
        let _ = std::fs::remove_file(&p);
        return Err("安装包已损坏或被替换，已拒绝执行".into());
    }

    launch_installer(&path, &std::env::current_exe().map_err(|e| e.to_string())?.display().to_string())?;
    // 稍退一步再退出：让前端收到本次回复，也给安装器腾出文件占用
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.exit(0);
    });
    Ok(())
}

#[cfg(windows)]
fn launch_installer(path: &str, relaunch: &str) -> Result<(), String> {
    // 三段：等 2 秒让本进程退出并释放 voiceassist.exe 占用 → 静默安装并等它结束
    // → 按原路径重新拉起（不等安装完就拉起会拿到旧进程或撞上占用）。
    // 用 Start-Sleep 而非 timeout——后者要求可读 stdin，隐藏窗口下会报错。
    let ps = format!(
        "Start-Sleep -Seconds 2; Start-Process -FilePath '{}' -ArgumentList '/S' -Wait; Start-Process -FilePath '{}'",
        path.replace('\'', "''"),
        relaunch.replace('\'', "''")
    );
    crate::proc::hidden_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps,
        ])
        .spawn()
        .map_err(|e| format!("拉起安装器失败：{e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_installer(_path: &str, _relaunch: &str) -> Result<(), String> {
    Err("当前平台不支持应用内升级，请手动下载安装".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_manifest() -> AppUpdateManifest {
        let sha = "a".repeat(64);
        AppUpdateManifest {
            version: "9.9.9".into(),
            file: "TTSassist_9.9.9_x64-setup.exe".into(),
            size: 123,
            sha256: sha,
            notes: "## 说明".into(),
            gitee_url: "https://gitee.com/yihwan/TTSassist/releases/download/v9.9.9/TTSassist_9.9.9_x64-setup.exe".into(),
            github_url: "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v9.9.9/TTSassist_9.9.9_x64-setup.exe".into(),
        }
    }

    #[test]
    fn 合法清单校验通过并可下载() {
        let m = good_manifest();
        assert!(m.validate_download().is_ok());
        let info = manifest_to_info(&m).unwrap().expect("9.9.9 应视为有更新");
        assert!(info.has_download);
        assert_eq!(info.version, "9.9.9");
        assert!(info.url.ends_with("/tag/v9.9.9"));
    }

    #[test]
    fn 非白名单域名被拒绝() {
        let mut m = good_manifest();
        m.gitee_url = "https://evil.example.com/x/TTSassist_9.9.9_x64-setup.exe".into();
        assert!(m.validate_download().unwrap_err().contains("白名单"));
    }

    #[test]
    fn 直链文件名与清单不符被拒() {
        let mut m = good_manifest();
        m.github_url = "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v9.9.9/TTSassist_1.0.0_x64-setup.exe".into();
        assert!(m.validate_download().unwrap_err().contains("文件名"));
    }

    #[test]
    fn 文件名与版本号不一致被拒() {
        let mut m = good_manifest();
        m.file = "TTSassist_1.0.0_x64-setup.exe".into();
        assert!(m.validate_download().unwrap_err().contains("文件名异常"));
    }

    #[test]
    fn 校验值形状与空直链均被拒() {
        let mut m = good_manifest();
        m.sha256 = "deadbeef".into();
        assert!(m.validate_download().unwrap_err().contains("sha256"));

        let mut m2 = good_manifest();
        m2.gitee_url.clear();
        m2.github_url.clear();
        assert!(m2.validate_download().unwrap_err().contains("直链"));
    }

    #[test]
    fn 通道顺序国内优先且空地址剔除() {
        let mut m = good_manifest();
        m.gitee_url.clear();
        let chans = m.channels();
        assert_eq!(chans.len(), 1);
        assert_eq!(chans[0].0, "github");

        let m = good_manifest();
        let chans = m.channels();
        assert_eq!(chans.len(), 2);
        assert_eq!(chans[0].0, "gitee");
        assert_eq!(chans[1].0, "github");
    }

    #[test]
    fn 已是最新时不报更新() {
        let cur = env!("CARGO_PKG_VERSION");
        let mut m = good_manifest();
        m.version = cur.into();
        m.file = format!("TTSassist_{cur}_x64-setup.exe");
        assert!(manifest_to_info(&m).unwrap().is_none());
    }

    /// 线上清单自检：默认跳过，每次发完版手动跑一次（`cargo test --lib -- --ignored`）。
    /// 它走的是真实 fetch_manifest + validate_download，能在装机器之前
    /// 就发现清单字段漂移、域名写错、安装包没传上去这类发布事故。
    #[test]
    #[ignore = "需要联网，发版后手动执行"]
    fn 线上清单可拉取且通过校验() {
        let m = tauri::async_runtime::block_on(fetch_manifest()).expect("线上清单应可拉取");
        m.validate_download().expect("线上清单应通过白名单与自洽校验");
        assert_eq!(m.version.split('.').count(), 3, "版本号应形如 x.y.z");
        assert!(is_app_version(&m.version));
        assert!(!m.notes.trim().is_empty(), "清单应携带更新说明");
        // 刚发完版时清单版本应等于或高于当前包；低于当前包说明 dist 没更新
        let cur = env!("CARGO_PKG_VERSION");
        assert!(
            !crate::plugins::manifest::version_less_than(&m.version, cur),
            "线上清单版本 {} 不应低于当前包版本 {}",
            m.version,
            cur
        );
    }

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
