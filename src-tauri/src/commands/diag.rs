// 诊断包一键导出（T1）：把排障所需的本机状态收集成一个人可读的 .txt。
//
// ⚠ 隐私红线（本模块的存在意义，改动前先读）：
//   - 绝不做任何网络上传：本命令只在本机生成一个文件，无任何出网行为；
//   - 不自动打开文件夹、不请求管理员权限；
//   - 默认不含用户文本：generate_tts 入参、消息正文、收藏标题、自定义音色名
//     一律不进诊断包（采集层面直接不取）；
//   - 日志尾部逐行经 diag_redact::redact_line 二次脱敏 + 200 字符硬截断。
//
// 可靠性约定：任何一节失败（含 panic）只写 unavailable: <原因>，整命令不报错。

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::AppState;
use crate::diag_redact::redact_line;

/// 导出结果（camelCase 与前端 src/types 风格一致）
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagExportResult {
    pub path: String,
    pub bytes: u64,
    pub sections: u32,
}

/// 单节采集保护：失败/panic 只降级为 unavailable 行，不拖垮整命令。
fn safe_section(f: impl FnOnce() -> String) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(s) => s,
        Err(_) => "unavailable: 该节采集时发生内部错误".to_string(),
    }
}

fn section(body: Vec<String>) -> String {
    let mut out = body.join("\n");
    out.push('\n');
    out
}

// ── 版本节 ──────────────────────────────────────────

/// OS 版本串：`cmd /c ver`（零依赖；无控制台黑框，见 proc::hidden_command）。
/// 中文系统的 ver 输出是 GBK（如「版本」二字），UTF-8 解码后成乱码，
/// 这里只提取形如 10.0.26100.3025 的数字版本号，其余丢弃。
fn os_version_line() -> String {
    let out = crate::proc::hidden_command("cmd")
        .args(["/c", "ver"])
        .output();
    match out {
        Ok(o) => {
            let raw = String::from_utf8_lossy(&o.stdout);
            match extract_version_numbers(&raw) {
                Some(v) => format!("Windows {v}"),
                None => "unavailable: ver 输出未解析出版本号".into(),
            }
        }
        Err(e) => format!("unavailable: {e}"),
    }
}

/// 从（可能被编码污染的）文本中提取最长的 `d+.d+.d+` 形态版本号（纯 ASCII 扫描）。
fn extract_version_numbers(s: &str) -> Option<String> {
    let valid = |cur: &str| {
        cur.matches('.').count() >= 2 && cur.starts_with(|c: char| c.is_ascii_digit())
    };
    let mut best: Option<String> = None;
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            cur.push(c);
        } else {
            if valid(&cur) && best.as_ref().map(|b| cur.len() > b.len()).unwrap_or(true) {
                best = Some(cur.clone());
            }
            cur.clear();
        }
    }
    if valid(&cur) && best.as_ref().map(|b| cur.len() > b.len()).unwrap_or(true) {
        best = Some(cur);
    }
    best
}

/// 安装形态：在 Uninstall 注册表树里搜应用标识，命中即安装版，否则便携版。
fn install_kind_line() -> String {
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    for hive in ["HKCU", "HKLM"] {
        let ok = crate::proc::hidden_command("reg")
            .args(["query", &format!("{hive}\\{key}"), "/f", "com.voiceassist.app", "/s", "/k"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if ok {
            return "安装版（注册表有卸载项）".into();
        }
    }
    "便携版（注册表无卸载项）".into()
}

fn render_versions(app: &AppHandle) -> String {
    let pkg = app.package_info();
    section(vec![
        "## 版本".into(),
        format!("app: v{} ({})", pkg.version, pkg.name),
        format!("tauri_core: {}", tauri::VERSION),
        format!("os: {} / arch: {}", os_version_line(), std::env::consts::ARCH),
        format!("安装形态: {}", install_kind_line()),
    ])
}

// ── 路径节 ──────────────────────────────────────────

fn render_paths(state: &State<AppState>) -> String {
    let log_line = match crate::logging::log_file_path() {
        Some(p) => {
            let (size, mtime) = match std::fs::metadata(&p) {
                Ok(m) => (
                    format!("{} bytes", m.len()),
                    m.modified()
                        .ok()
                        .map(|t| {
                            let dt: chrono::DateTime<chrono::Local> = t.into();
                            dt.format("%Y-%m-%d %H:%M:%S").to_string()
                        })
                        .unwrap_or_else(|| "unknown".into()),
                ),
                Err(_) => ("文件尚未创建".into(), String::new()),
            };
            format!("{}（{} {}）", p.display(), size, mtime)
        }
        None => "unavailable: 日志模块尚未初始化".into(),
    };
    section(vec![
        "## 路径".into(),
        format!("data_dir: {}", state.data_dir.display()),
        format!("日志文件: {log_line}"),
        format!("诊断日志开关: {}", if crate::logging::is_enabled() { "开启" } else { "关闭" }),
    ])
}

// ── 音频节 ──────────────────────────────────────────

fn render_audio() -> String {
    // 直接复用 mic.rs 的设备枚举（不许另写一份 cpal 枚举代码）
    let devices = crate::commands::mic::list_output_devices();
    let default = devices
        .iter()
        .find(|d| d.is_default)
        .map(|d| d.name.clone());
    let mut lines = vec!["## 音频".into()];
    lines.push(match default {
        Some(n) => format!("默认输出设备: {n}"),
        None => "默认输出设备: unavailable: 枚举不到默认设备".into(),
    });
    if devices.is_empty() {
        lines.push("输出设备列表: unavailable: 枚举为空".into());
    } else {
        lines.push(format!("输出设备列表（{} 个，输出/输入均指 playback 设备，mic.rs 仅提供输出枚举）:", devices.len()));
        for d in &devices {
            let mut tag = Vec::new();
            if d.is_default {
                tag.push("默认");
            }
            if d.is_virtual_cable {
                tag.push("VB-CABLE");
            }
            let tag = if tag.is_empty() { String::new() } else { format!(" [{}]", tag.join(",")) };
            lines.push(format!("  - {}{tag}", d.name));
        }
    }
    let has_cable = devices.iter().any(|d| d.is_virtual_cable);
    lines.push(format!("VB-CABLE 在场: {}", if has_cable { "是" } else { "否" }));
    section(lines)
}

// ── 插件节 ──────────────────────────────────────────

fn render_plugins(app: &AppHandle) -> String {
    let mut lines = vec!["## 插件".into()];
    let manager = app.try_state::<crate::plugins::PluginManager>();
    match manager {
        Some(manager) => {
            let rows = manager.diag_state();
            if rows.is_empty() {
                lines.push("（注册表为空，未安装任何插件）".into());
            }
            for (id, version, loaded, error) in rows {
                let status = if loaded { "已加载".to_string() } else { "未加载".to_string() };
                lines.push(format!("  - {id} v{version}: {status}"));
                if let Some(err) = error {
                    lines.push(format!("    加载失败原因: {err}"));
                }
            }
        }
        None => lines.push("unavailable: 插件管理器未初始化".into()),
    }
    // 线上插件索引仅在线拉取，本地无缓存文件（数据源不存在，如实写）
    lines.push("plugins-index.json 缓存版本: unavailable: 索引为在线拉取，本地无缓存文件".into());
    section(lines)
}

// ── 遥关节 ──────────────────────────────────────────

fn render_remote(app: &AppHandle, include_host: bool) -> String {
    // 子进程/TCP 探测放当前命令线程即可（诊断为低频手动操作）
    let listening = crate::commands::remote::port_listening();
    let firewall = std::thread::spawn(crate::commands::remote::firewall_rule_present);
    let mut lines = vec!["## 遥控".into()];
    lines.push(format!(
        "服务运行: {}",
        if listening { "是（TCP 45271 本机可连）" } else { "否（TCP 45271 不可连）" }
    ));
    lines.push(format!("TCP 45271 监听: {}", if listening { "是" } else { "否" }));
    let firewall = firewall.join().unwrap_or(false);
    lines.push(format!(
        "防火墙规则（VoiceAssist Remote TCP 45271）: {}",
        if firewall { "存在" } else { "不存在" }
    ));
    lines.push(match crate::commands::remote::egress_ip() {
        Some(ip) => format!("出口 IP: {ip}"),
        None => "出口 IP: unavailable: 探测失败（无网络路由）".into(),
    });
    match app.try_state::<crate::remote::RemoteCore>() {
        Some(core) => {
            let info = core.session_info();
            lines.push(format!(
                "已连接设备: {}",
                if info.connected {
                    let peer = if include_host {
                        format!("（对端 {}）", info.peer.as_deref().unwrap_or("unknown"))
                    } else {
                        "（对端信息未勾选包含，省略）".into()
                    };
                    format!("有{peer}")
                } else {
                    "无".into()
                }
            ));
            lines.push(format!(
                "配对: {}",
                if info.paired {
                    // 设备名（手机型号/用户命名）归「包含机器名」复选，默认不采
                    match (&info.device, include_host) {
                        (Some(_), true) => format!("已配对（{}）", info.device.as_deref().unwrap_or("未知设备名")),
                        (Some(_), false) => "已配对（设备名未勾选包含，省略）".into(),
                        (None, _) => "已配对".into(),
                    }
                } else {
                    "未配对".into()
                }
            ));
        }
        None => lines.push("会话信息: unavailable: RemoteCore 未初始化".into()),
    }
    match crate::remote::mdns::registration_state() {
        Some(Ok(())) => lines.push("mDNS: 已注册广播（_ttsassist-remote._tcp）".into()),
        Some(Err(e)) => lines.push(format!("mDNS: 未注册（{e}）")),
        None => lines.push("mDNS: unavailable: 广播线程尚未上报状态".into()),
    }
    section(lines)
}

// ── 快捷键节 ────────────────────────────────────────

/// 单个热键行：配置值 + 注册状态（配置非空但状态空 = 注册失败）。
fn hotkey_line(label: &str, configured: &str, registered: Option<&Option<String>>) -> String {
    let (value, status) = match registered {
        Some(Some(accel)) => (accel.clone(), "已注册"),
        _ => {
            if configured.trim().is_empty() {
                (String::new(), "未设置")
            } else {
                (configured.to_string(), "注册失败")
            }
        }
    };
    if value.is_empty() {
        format!("  - {label}: （未设置）")
    } else {
        format!("  - {label}: {value} — {status}")
    }
}

fn render_hotkeys(app: &AppHandle, state: &State<AppState>) -> String {
    let mut lines = vec!["## 快捷键".into()];
    let s = match state.settings.read() {
        Ok(s) => s.clone(),
        Err(_) => {
            return section(vec!["## 快捷键".into(), "unavailable: 设置读取失败".into()]);
        }
    };
    macro_rules! line_for {
        ($label:expr, $configured:expr, $state_ty:ty) => {{
            let cur = app
                .try_state::<$state_ty>()
                .and_then(|st| st.current.lock().ok().map(|g| g.clone()));
            hotkey_line($label, &$configured, cur.as_ref())
        }};
    }
    lines.push(line_for!("显示小窗（呼出浮窗）", s.hotkey_show_window, crate::hotkey::HotkeyState));
    lines.push(line_for!("语音输入", s.voice_input_hotkey, crate::hotkey::VoiceInputHotkeyState));
    lines.push(line_for!("重播上一条", s.hotkey_play_last, crate::hotkey::PlayLastHotkeyState));
    lines.push(line_for!("麦克风控（发送开关）", s.hotkey_mic_toggle, crate::hotkey::MicToggleHotkeyState));
    lines.push(line_for!("字幕暂停", s.subtitle_pause_hotkey, crate::hotkey::SubtitlePauseHotkeyState));
    match app.try_state::<crate::hotkey::FavoriteHotkeys>() {
        Some(fav) => match fav.registered.lock() {
            Ok(set) => {
                if set.is_empty() {
                    lines.push("  - 收藏快捷键: （无）".into());
                } else {
                    let mut keys: Vec<String> = set.iter().cloned().collect();
                    keys.sort();
                    // 隐私：只列键位串，不列收藏标题
                    lines.push(format!("  - 收藏快捷键（{} 个，仅键位）: {}", keys.len(), keys.join(", ")));
                }
            }
            Err(_) => lines.push("  - 收藏快捷键: unavailable: 状态锁读取失败".into()),
        },
        None => lines.push("  - 收藏快捷键: unavailable: 状态未初始化".into()),
    }
    section(lines)
}

// ── 设置摘要节 ──────────────────────────────────────

/// 仅布尔/枚举/数字字段 + 点名的引擎/音色 id。
/// 文本字段（API Key、克隆音色名/路径、设备名、进程名、plugin_config）一律不采集。
fn render_settings_summary(state: &State<AppState>) -> String {
    let s = match state.settings.read() {
        Ok(s) => s.clone(),
        Err(_) => return section(vec!["## 设置摘要".into(), "unavailable: 设置读取失败".into()]),
    };
    let mut lines = vec!["## 设置摘要".into()];
    // 枚举 / id
    lines.push(format!("  - tts_engine: {}", s.tts_engine));
    lines.push(format!("  - tts_model（音色 id）: {}", s.tts_model));
    lines.push(format!("  - engine_category: {}", s.engine_category));
    lines.push(format!("  - moss_voice_id: {}", s.moss_voice_id));
    lines.push(format!("  - asr_plugin: {}", s.asr_plugin));
    lines.push(format!("  - asr_language: {}", s.asr_language));
    lines.push(format!("  - theme: {}", s.theme));
    lines.push(format!("  - floating_ball_perf_mode: {}", s.floating_ball_perf_mode));
    lines.push(format!("  - floating_ball_skin: {}", s.floating_ball_skin));
    lines.push(format!("  - subtitle_position: {}", s.subtitle_position));
    lines.push(format!("  - subtitle_language: {}", s.subtitle_language));
    lines.push(format!("  - subtitle_vad_sensitivity: {}", s.subtitle_vad_sensitivity));
    // 数字
    lines.push(format!("  - playback_volume: {:.2}", s.playback_volume));
    lines.push(format!("  - playback_rate: {:.2}", s.playback_rate));
    lines.push(format!("  - mic_playback_volume: {:.2}", s.mic_playback_volume));
    lines.push(format!("  - floating_ball_size: {}", s.floating_ball_size));
    lines.push(format!("  - subtitle_opacity: {:.2}", s.subtitle_opacity));
    lines.push(format!("  - subtitle_font_size: {}", s.subtitle_font_size));
    lines.push(format!("  - subtitle_max_lines: {}", s.subtitle_max_lines));
    lines.push(format!("  - subtitle_fade_seconds: {}", s.subtitle_fade_seconds));
    // 布尔
    lines.push(format!("  - mic_send_enabled: {}", s.mic_send_enabled));
    lines.push(format!("  - voice_input_enabled: {}", s.voice_input_enabled));
    lines.push(format!("  - floating_ball_enabled: {}", s.floating_ball_enabled));
    lines.push(format!("  - diagnostics_log_enabled: {}", s.diagnostics_log_enabled));
    lines.push(format!("  - subtitle_enabled: {}", s.subtitle_enabled));
    lines.push(format!("  - subtitle_always_on_top: {}", s.subtitle_always_on_top));
    // 插件音色选择（id → id，排障必需；不含音色显示名）
    if s.plugin_voices.is_empty() {
        lines.push("  - plugin_voices: （无）".into());
    } else {
        let mut keys: Vec<&String> = s.plugin_voices.keys().collect();
        keys.sort();
        for k in keys {
            lines.push(format!("  - plugin_voices[{}]: {}", k, s.plugin_voices[k]));
        }
    }
    section(lines)
}

// ── 日志尾部节 ──────────────────────────────────────

const LOG_TAIL_LINES: usize = 200;

/// 收集候选主机名（COMPUTERNAME / HOSTNAME 及 mdns 实例名同源的 32 字符截断变体）。
fn candidate_hostnames() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(h) = std::env::var(key) {
            let h = h.trim().to_string();
            if !h.is_empty() {
                out.push(h.clone());
                let short: String = h.chars().take(32).collect();
                if short != h {
                    out.push(short);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 未勾选「包含机器名」时，把日志行中的主机名替换为占位符（防 mDNS 等行带出主机名）。
fn scrub_hosts(line: String, hosts: &[String]) -> String {
    let mut out = line;
    for h in hosts {
        if !h.is_empty() {
            out = out.replace(h.as_str(), "<redacted-host>");
        }
    }
    out
}

fn render_log_tail(include_host: bool) -> String {
    let mut lines = vec!["## 日志尾部".into()];
    match crate::logging::log_file_path().and_then(|p| std::fs::read_to_string(&p).ok()) {
        Some(content) => {
            let all: Vec<&str> = content.lines().collect();
            let start = all.len().saturating_sub(LOG_TAIL_LINES);
            lines.push(format!(
                "（最后 {} 行，已经脱敏 + 200 字符截断{}）",
                all.len() - start,
                if include_host { "" } else { "；主机名已替换" }
            ));
            let hosts = if include_host { Vec::new() } else { candidate_hostnames() };
            for l in &all[start..] {
                lines.push(scrub_hosts(redact_line(l), &hosts));
            }
        }
        None => lines.push("diagnostics log is off".into()),
    }
    section(lines)
}

// ── 命令本体 ────────────────────────────────────────

/// 采集 → 渲染 → save 面板选路径 → 落盘（UTF-8 无 BOM）→ 返回路径。
/// `include_host`：是否包含机器名/设备名（归确认面板复选，默认 false）。
/// 用户取消保存时返回 Err("已取消")，前端据此静默复位（不算失败态）。
#[tauri::command]
pub fn export_diagnostics(
    app: AppHandle,
    state: State<AppState>,
    include_host: Option<bool>,
) -> Result<DiagExportResult, String> {
    let include_host = include_host.unwrap_or(false);
    // 逐节采集（每节独立兜底）
    let versions = safe_section(|| render_versions(&app));
    let paths = safe_section(|| render_paths(&state));
    let audio = safe_section(render_audio);
    let plugins = safe_section(|| render_plugins(&app));
    let remote = safe_section(|| render_remote(&app, include_host));
    let hotkeys = safe_section(|| render_hotkeys(&app, &state));
    let settings = safe_section(|| render_settings_summary(&state));
    let log_tail = safe_section(|| render_log_tail(include_host));

    let head = format!(
        "VoiceAssist 诊断信息（本文件仅存于你的电脑，导出过程没有任何上传）\n生成时间: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    );
    let body = format!("{head}{versions}{paths}{audio}{plugins}{remote}{hotkeys}{settings}{log_tail}");
    let sections: u32 = 8;

    // 默认文件名：va-diag-<版本>-<yyyyMMdd-HHmmss>.txt；默认落「下载」目录
    let version = app.package_info().version.to_string();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let default_name = format!("va-diag-{version}-{ts}.txt");

    let mut dialog = app.dialog().file().add_filter("文本文件", &["txt"]).set_file_name(&default_name);
    if let Ok(dl) = app.path().download_dir() {
        dialog = dialog.set_directory(dl);
    }
    let picked = dialog
        .blocking_save_file()
        .ok_or_else(|| "已取消".to_string())?;
    let path = picked
        .into_path()
        .map_err(|e| format!("保存路径无效: {e}"))?;

    // UTF-8 无 BOM（std::fs::write 直接写字节，严禁在此用带 BOM 的编码写入）
    let bytes = body.as_bytes().len() as u64;
    std::fs::write(&path, body.as_bytes()).map_err(|e| format!("写入诊断文件失败: {e}"))?;

    log_info!("[diag] 诊断包已导出: {}（{bytes} bytes，{sections} 节）", path.display());
    Ok(DiagExportResult {
        path: path.to_string_lossy().into_owned(),
        bytes,
        sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端脱敏验证（§3.6 手工验证的自动化等效项）：
    /// 真实日志链路写入含用户文本的行 → render_log_tail 产出 → 原文必须搜不到。
    #[test]
    fn 日志尾部节不含用户合成文本() {
        let tmp = tempfile::tempdir().unwrap();
        // 初始化到临时目录（OnceLock 全局生效一次；本测试独立于其他测试的断言）
        crate::logging::init(&tmp.path().to_path_buf(), true);
        // 模拟有人把用户文本打进了日志（这正是二次防线要拦的）
        log_info!("合成完成 text=\"今天股票会涨吗救救我\" engine=mimo");
        log_info!("识别结果: 内容=请把空调打开一点");
        let tail = render_log_tail(false);
        assert!(!tail.contains("股票"), "诊断包日志尾部不得含用户文本原词");
        assert!(!tail.contains("空调"), "诊断包日志尾部不得含用户文本原词");
        assert!(tail.contains("<redacted len="), "应出现脱敏占位");
        assert!(tail.contains("合成完成"), "非敏感骨架行应保留");
    }

    #[test]
    fn 版本号提取_兼容gbk乱码与英文输出() {
        // 中文系统 cmd /c ver 的 GBK 输出经 from_utf8_lossy 后「版本」成替换符
        assert_eq!(
            extract_version_numbers("Microsoft Windows [\u{FFFD}\u{FFFD} 10.0.26100.3025]"),
            Some("10.0.26100.3025".into())
        );
        assert_eq!(
            extract_version_numbers("Microsoft Windows [Version 10.0.19045.4046]"),
            Some("10.0.19045.4046".into())
        );
        // 纯点号串与单段数字不算版本号
        assert_eq!(extract_version_numbers("... 123 ab"), None);
        // 取最长匹配
        assert_eq!(
            extract_version_numbers("v1.2.3 tail 10.0.22631.1"),
            Some("10.0.22631.1".into())
        );
    }

    #[test]
    fn 主机名替换_含截断变体与不误伤() {
        let hosts = vec!["FISHAWDK".to_string(), "FISHAWDK-PC".to_string()];
        let out = scrub_hosts(
            "[remote] mDNS 广播已启动: _ttsassist-remote._tcp.local. FISHAWDK 192.168.1.188:45271".into(),
            &hosts,
        );
        assert!(out.contains("<redacted-host>"));
        assert!(!out.contains("FISHAWDK"));
        // 不含主机名的行原样
        let line2 = "[info] done=ok".to_string();
        assert_eq!(scrub_hosts(line2.clone(), &hosts), line2);
    }
}
