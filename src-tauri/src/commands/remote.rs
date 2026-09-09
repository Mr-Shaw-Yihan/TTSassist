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

// ── 局域网遥控可达性：自检 + 防火墙一键放行 ────────────────
// 背景：免安装 zip 版 / 安装时 UAC 被取消 / 老版本安装的用户，Windows 防火墙
// 没放行 TCP 45271，导致手机 App 与网页都无法建立入站连接（表现为“都连不上”）。
// 这里让设置面板能自查（监听/防火墙/本机各网卡 IP）并一键补上放行规则。

/// 局域网可达性自检结果
#[derive(serde::Serialize)]
pub struct LanStatus {
    pub ips: Vec<LanIp>,
    pub listening: bool,
    pub firewall: bool,
    pub port: u16,
}

#[derive(serde::Serialize)]
pub struct LanIp {
    pub iface: String,
    pub ip: String,
    /// 是否为默认路由出口网卡（最可能就是手机能到的那块）
    pub default: bool,
}

/// 本机对外局域网 IP（UDP connect 技巧，取默认路由出口，无实际流量）
fn egress_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip())
}

/// 端口是否可连（原生 TCP 连回环，绕开 PowerShell/cmdlet 依赖，最稳）
fn port_listening() -> bool {
    use std::io::ErrorKind;
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let addr = match ("127.0.0.1", 45271u16).to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(a) => a,
        None => return false,
    };
    match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
        Ok(_) => true,
        Err(e) => !matches!(e.kind(), ErrorKind::ConnectionRefused),
    }
}

/// 防火墙是否已放行 TCP 45271（netsh 输出含本规则 ASCII 名即认为存在，跨语言稳健）
fn firewall_rule_present() -> bool {
    const RULE: &str = "VoiceAssist Remote TCP 45271";
    std::process::Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", &format!("name={RULE}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(RULE))
        .unwrap_or(false)
}

/// 枚举本机非环回/非链路本地 IPv4（含网卡名）。PowerShell 仅作增强，
/// 取不到时由调用方用默认出口 IP 兜底。
fn lan_ips() -> Vec<LanIp> {
    let script = "$ErrorActionPreference='SilentlyContinue';@{ips=@(Get-NetIPAddress -AddressFamily IPv4 | ? { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | % { @{ iface=$_.InterfaceAlias; ip=$_.IPAddress } })} | ConvertTo-Json -Compress";
    let egress = egress_ip().map(|a| a.to_string());
    let mut ips: Vec<LanIp> = Vec::new();
    if let Ok(out) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(arr) = v.get("ips").and_then(|x| x.as_array()) {
                for it in arr {
                    let ip = it.get("ip").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    if ip.is_empty() {
                        continue;
                    }
                    let iface = it.get("iface").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let default = egress.as_deref() == Some(ip.as_str());
                    ips.push(LanIp { iface, ip, default });
                }
            }
        }
    }
    ips.sort_by(|a, b| b.default.cmp(&a.default));
    ips
}

/// 查询局域网遥控状态：网卡 IPv4 列表 + 端口监听 + 防火墙规则。
/// 监听/防火墙走原生探测（不依赖 PowerShell cmdlet，最稳）；网卡列表取不到时以出口 IP 兜底。
#[tauri::command]
pub async fn remote_lan_status() -> Result<LanStatus, String> {
    tokio::task::spawn_blocking(|| {
        let mut ips = lan_ips();
        if ips.is_empty() {
            if let Some(ip) = egress_ip().map(|a| a.to_string()) {
                ips.push(LanIp {
                    iface: String::new(),
                    ip,
                    default: true,
                });
            }
        }
        Ok(LanStatus {
            ips,
            listening: port_listening(),
            firewall: firewall_rule_present(),
            port: 45271,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 一键放行：提权执行与安装器相同的 netsh 规则（TCP 45271 + UDP 5353，profile=any）。
/// 弹一次 UAC；用户取消则回明确提示。netsh 写入临时 .cmd 避免嵌套引号问题。
#[tauri::command]
pub async fn remote_firewall_open() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let netsh = "netsh advfirewall firewall delete rule name=\"VoiceAssist Remote TCP 45271\" >nul 2>&1\r\nnetsh advfirewall firewall add rule name=\"VoiceAssist Remote TCP 45271\" dir=in action=allow protocol=TCP localport=45271 profile=any\r\nnetsh advfirewall firewall delete rule name=\"VoiceAssist mDNS UDP 5353\" >nul 2>&1\r\nnetsh advfirewall firewall add rule name=\"VoiceAssist mDNS UDP 5353\" dir=in action=allow protocol=UDP localport=5353 profile=any";
        let body = format!("@echo off\r\n{}\r\n", netsh);
        let bat = std::env::temp_dir().join("voiceassist_remote_firewall.cmd");
        std::fs::write(&bat, body.as_bytes()).map_err(|e| format!("写入临时脚本失败: {e}"))?;
        let bat_str = bat.to_string_lossy().replace('\'', "");
        let ps = format!(
            "Start-Process -FilePath '{}' -Verb RunAs -WindowStyle Hidden -Wait",
            bat_str
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output();
        let _ = std::fs::remove_file(&bat);
        let out = out.map_err(|e| format!("启动失败: {e}"))?;
        if out.status.success() {
            Ok("防火墙规则已写入（TCP 45271 / UDP 5353，覆盖公用/专用网络）。".into())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("canceled") || stderr.contains("取消") || stderr.contains("0x800704C7") {
                Err("已取消管理员授权，未修改防火墙。放行需要管理员权限。".into())
            } else {
                Err(format!("放行失败: {}", stderr.trim()))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
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
