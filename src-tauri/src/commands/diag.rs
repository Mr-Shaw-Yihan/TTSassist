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

fn render_paths(data_dir: &std::path::Path) -> String {
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
        format!("data_dir: {}", data_dir.display()),
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
        // 第一轮遗留文案订正：本节下面就有「### 输入设备（录音）」小节，
        // 「mic.rs 仅提供输出枚举」已不成立，会误导读包的人。
        lines.push(format!("输出设备列表（{} 个，playback 设备）:", devices.len()));
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
    // 输入（录音）设备（T-B）：ASR「听不到/识别不了」类报障的关键信息
    lines.push("### 输入设备（录音）".into());
    match std::panic::catch_unwind(crate::commands::mic::list_input_devices) {
        Ok(inputs) => {
            if inputs.is_empty() {
                lines.push("- (未检测到输入设备)".into());
            }
            for d in &inputs {
                lines.push(format!(
                    "- {} | default={} | vb-cable={}",
                    d.name, d.is_default, d.is_virtual_cable
                ));
            }
        }
        Err(_) => lines.push("- unavailable: 输入设备枚举发生内部错误".into()),
    }
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
    // 插件音色选择（T-A 追加收口）：**只报哪些引擎配了音色，不报值**。
    // 值是用户自由文本——MinimaxVoicePanel 把用户自命名的克隆音色名直接写进
    // plugin_voices（实测导出包出现过 `doorman0816`），等于从侧门把用户文本带进诊断包，
    // 违反本函数开头「克隆音色名一律不采集」的声明。排障只需知道"有没有配"。
    if s.plugin_voices.is_empty() {
        lines.push("  - plugin_voices: （无）".into());
    } else {
        let mut keys: Vec<&String> = s.plugin_voices.keys().collect();
        keys.sort();
        let ids: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();
        lines.push(format!(
            "  - plugin_voices 已配置引擎: {}（音色值可能为用户自命名，一律省略）",
            ids.join(", ")
        ));
    }
    section(lines)
}

// ── 身份标识脱敏（T-A：整份 body 统一 scrub）────────

/// 收集身份候选，返回 (主机/机器名候选, 用户名候选)。
/// 主机名（COMPUTERNAME / HOSTNAME 及 mdns 实例名同源的 32 字符截断变体）
/// 受「包含机器名」复选控制；用户名（USERNAME / USERPROFILE 的用户目录段）
/// **永远脱敏，不在复选范围内**（第一轮 §3.4 C 红线的用户名部分，T-A 收口）。
/// `data_dir` 的用户段是第三重来源：env 在异常环境（服务化、精简会话）可能读不到，
/// 那时 USERNAME/USERPROFILE 双双落空会让整条用户名防线**静默失效**，而诊断包里的
/// 路径节必然含 `...\Users\<账户名>\...`——它才是最可靠的候选来源。
fn candidate_identities(data_dir: &std::path::Path) -> (Vec<String>, Vec<String>) {
    let mut hosts: Vec<String> = Vec::new();
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(h) = std::env::var(key) {
            push_identity(&mut hosts, &h);
        }
    }
    let mut users: Vec<String> = Vec::new();
    if let Ok(u) = std::env::var("USERNAME") {
        push_identity(&mut users, &u);
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        // C:\Users\<账户名>\... → 取用户目录名段
        if let Some(seg) = std::path::Path::new(&profile).file_name() {
            push_identity(&mut users, &seg.to_string_lossy());
        }
    }
    if let Some(seg) = users_dir_segment(data_dir) {
        push_identity(&mut users, &seg);
    }
    (hosts, users)
}

/// 取路径中 `Users\<段>` 或 `home/<段>` 的那一段（Windows 与 Linux/macOS 双口径）。
/// 找不到返回 None，绝不 panic——非标准安装目录（`D:\App\data`）是常态。
fn users_dir_segment(path: &std::path::Path) -> Option<String> {
    let mut comps = path.components();
    while let Some(c) = comps.next() {
        if let std::path::Component::Normal(seg) = c {
            if seg.to_string_lossy().eq_ignore_ascii_case("users")
                || seg.to_string_lossy().eq_ignore_ascii_case("home")
            {
                return match comps.next() {
                    Some(std::path::Component::Normal(next)) => {
                        Some(next.to_string_lossy().into_owned())
                    }
                    _ => None,
                };
            }
        }
    }
    None
}

fn push_identity(out: &mut Vec<String>, raw: &str) {
    let h = raw.trim().to_string();
    if !h.is_empty() {
        out.push(h.clone());
        // mdns 实例名与 COMPUTERNAME 同源，截断到 32 字符（对齐 remote/mdns.rs hostname()）
        let short: String = h.chars().take(32).collect();
        if short != h {
            out.push(short);
        }
    }
}

/// 由候选生成替换清单：去重（大小写不敏感口径）、过滤空串与长度 <2 的候选，
/// 按长度降序排列（长候选先替换，防止 "FISHAWDK" 先于 "FISHAWDK-PC" 替换留下 "-PC" 残尾）。
/// 大小写变体（ABC/abc/AbC…）由 replace_ci 的大小写不敏感匹配统一覆盖，不再逐个展开
/// 形态——那是任务书原定「三形态」方案的超集（三形态覆盖不了 AbC 混合写法）。
/// 空串与长度 <2 的候选直接丢弃——`replace("", X)` 会在每个字符之间插 X，
/// 把整份文本炸成不可读垃圾（T-A 实现红线）。
fn identity_variants(cands: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in cands {
        let c = c.trim();
        if c.chars().count() < 2 {
            continue;
        }
        if !out.iter().any(|v| v.eq_ignore_ascii_case(c)) {
            out.push(c.to_string());
        }
    }
    out.sort_by_key(|h| std::cmp::Reverse(h.chars().count()));
    out
}

/// 大小写不敏感（ASCII 范围）的**整词**替换；中文等非 ASCII 字符按原样参与相等比较。
/// needle 为空时原样返回（防逐字符插入污染）。零依赖手写扫描，不引 regex。
/// 词边界（T-A 追加加固）：账户名常常只有 2~3 个字符，无边界子串替换会把
/// `plugin_id`、`subtitle_max_lines` 这类报障要看的关键字打糊成垃圾。
/// 因此命中片段两侧不得是单词字符（ASCII 字母数字与 `_`）；`\` `/` `-` `.` `=` 空格
/// 中文等都算分隔符——路径 `C:\Users\<NAME>\`、设备名 `<NAME> 的麦克风`、主机名
/// `HOST-PC` 三类真实形态仍然照常命中。
fn replace_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let hay: Vec<char> = haystack.chars().collect();
    let pat: Vec<char> = needle.chars().collect();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < hay.len() {
        let hit = i + pat.len() <= hay.len()
            && hay[i..i + pat.len()]
                .iter()
                .zip(pat.iter())
                .all(|(a, b)| a.eq_ignore_ascii_case(b));
        if hit && word_bounded(&hay, i, pat.len()) {
            out.push_str(replacement);
            i += pat.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 命中片段 [start, start+len) 的两侧是否为词边界（串首/串尾算边界）。
fn word_bounded(hay: &[char], start: usize, len: usize) -> bool {
    let before_ok = start == 0 || !is_word_char(hay[start - 1]);
    let after = start + len;
    let after_ok = after == hay.len() || !is_word_char(hay[after]);
    before_ok && after_ok
}

/// 对整份诊断文本做身份脱敏：用户名候选无条件替换；主机名候选仅在
/// 未勾选「包含机器名」（include_host = false）时替换。
/// 统一挂在 export_diagnostics 的 body 汇总处——以后新增任何节都自动受管。
fn scrub_body(
    body: String,
    include_host: bool,
    host_cands: &[String],
    user_cands: &[String],
) -> String {
    let mut out = body;
    if !include_host {
        for h in identity_variants(host_cands) {
            out = replace_ci(&out, &h, "<redacted-host>");
        }
    }
    for u in identity_variants(user_cands) {
        out = replace_ci(&out, &u, "<redacted-host>");
    }
    out
}

const LOG_TAIL_LINES: usize = 200;

fn render_log_tail() -> String {
    let mut lines = vec!["## 日志尾部".into()];
    match crate::logging::log_file_path().and_then(|p| std::fs::read_to_string(&p).ok()) {
        Some(content) => {
            let all: Vec<&str> = content.lines().collect();
            let start = all.len().saturating_sub(LOG_TAIL_LINES);
            lines.push(format!(
                "（最后 {} 行，已经脱敏 + 200 字符截断；身份标识在整份汇总时统一脱敏）",
                all.len() - start
            ));
            for l in &all[start..] {
                lines.push(redact_line(l));
            }
        }
        None => lines.push("diagnostics log is off".into()),
    }
    section(lines)
}

// ── 命令本体 ────────────────────────────────────────

/// 采集 → 渲染 → save 面板选路径 → 落盘（UTF-8 无 BOM）→ 返回路径。
/// `include_host`：是否包含机器名（归确认面板复选，默认 false）；
/// 用户名不受该复选控制，永远脱敏。
/// 用户取消保存时返回 Err("已取消")，前端据此静默复位（不算失败态）。
#[tauri::command]
pub fn export_diagnostics(
    app: AppHandle,
    state: State<AppState>,
    include_host: Option<bool>,
) -> Result<DiagExportResult, String> {
    let include_host = include_host.unwrap_or(false);
    let (host_cands, user_cands) = candidate_identities(&state.data_dir);
    // 逐节采集（每节独立兜底）
    let versions = safe_section(|| render_versions(&app));
    let paths = safe_section(|| render_paths(&state.data_dir));
    let audio = safe_section(render_audio);
    let plugins = safe_section(|| render_plugins(&app));
    let remote = safe_section(|| render_remote(&app, include_host));
    let hotkeys = safe_section(|| render_hotkeys(&app, &state));
    let settings = safe_section(|| render_settings_summary(&state));
    let log_tail = safe_section(render_log_tail);

    let head = format!(
        "VoiceAssist 诊断信息（本文件仅存于你的电脑，导出过程没有任何上传）\n生成时间: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    );
    let body = format!("{head}{versions}{paths}{audio}{plugins}{remote}{hotkeys}{settings}{log_tail}");
    // 整份统一身份脱敏（T-A）：用户名永远替换；主机名按复选
    let body = scrub_body(body, include_host, &host_cands, &user_cands);
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

    // 自指导（T-A #5）：只打文件名——完整路径含账户名，下一次导出的日志尾部会把它带进诊断包
    let fname = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| default_name.clone());
    log_info!("[diag] 诊断包已导出: {fname}（{bytes} bytes，{sections} 节）");
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
        let tail = render_log_tail();
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
        let out = scrub_body(
            "[remote] mDNS 广播已启动: _ttsassist-remote._tcp.local. FISHAWDK 192.168.1.188:45271".into(),
            false,
            &hosts,
            &[],
        );
        assert_eq!(
            out,
            "[remote] mDNS 广播已启动: _ttsassist-remote._tcp.local. <redacted-host> 192.168.1.188:45271"
        );
        // 不含主机名的行原样
        let line2 = "[info] done=ok".to_string();
        assert_eq!(scrub_body(line2.clone(), false, &hosts, &[]), line2);
    }

    // ── T-A 七条 + 全链路共八条：身份标识收口（逐条独立，不许合并）──────

    #[test]
    fn 用户名从路径节被脱敏() {
        let paths = render_paths(std::path::Path::new(
            "C:\\Users\\unituser\\AppData\\Roaming\\com.voiceassist.app",
        ));
        let scrubbed = scrub_body(paths, false, &[], &["unituser".to_string()]);
        let data_line = scrubbed
            .lines()
            .find(|l| l.starts_with("data_dir:"))
            .expect("路径节必有 data_dir 行");
        assert_eq!(
            data_line,
            "data_dir: C:\\Users\\<redacted-host>\\AppData\\Roaming\\com.voiceassist.app"
        );
        assert!(!scrubbed.contains("unituser"));
    }

    #[test]
    fn 用户名经真实环境候选被脱敏() {
        // 全链路：candidate_identities() 读真实环境 → scrub 真实账户名。
        // USERNAME 与 USERPROFILE 段在标准 Windows 上同值（双来源冗余，单一来源
        // 被移除不致漏脱敏）；反测需同时移除两个来源才会变红。
        let user = std::env::var("USERNAME").unwrap_or_default();
        if user.trim().chars().count() < 2 {
            // 环境无用户名（极端 CI），本测试无从验证，静默通过
            return;
        }
        let dir = format!("C:\\Users\\{user}\\AppData\\Roaming\\com.voiceassist.app");
        let paths = render_paths(std::path::Path::new(&dir));
        let (hosts, users) = candidate_identities(std::path::Path::new(&dir));
        let scrubbed = scrub_body(paths, false, &hosts, &users);
        let data_line = scrubbed
            .lines()
            .find(|l| l.starts_with("data_dir:"))
            .expect("路径节必有 data_dir 行");
        assert_eq!(
            data_line,
            "data_dir: C:\\Users\\<redacted-host>\\AppData\\Roaming\\com.voiceassist.app"
        );
        assert!(!scrubbed.contains(&user));
    }

    #[test]
    fn 主机名大小写三形态均被替换() {
        let body = "行一 host=ABC\n行二 host=abc\n行三 host=AbC".to_string();
        let out = scrub_body(body, false, &["ABC".to_string()], &[]);
        assert_eq!(
            out,
            "行一 host=<redacted-host>\n行二 host=<redacted-host>\n行三 host=<redacted-host>"
        );
    }

    #[test]
    fn 长候选优先于短候选() {
        let cands = vec!["FISHAWDK".to_string(), "FISHAWDK-PC".to_string()];
        let out = scrub_body("dev=FISHAWDK-PC".into(), false, &cands, &[]);
        assert_eq!(out, "dev=<redacted-host>");
        assert!(!out.contains("-PC"), "不得留 -PC 残尾");
    }

    #[test]
    fn 设备名含账户名时被脱敏() {
        let body = "  - unituser 的麦克风 | default=false | vb-cable=false".to_string();
        let out = scrub_body(body, false, &[], &["unituser".to_string()]);
        assert_eq!(out, "  - <redacted-host> 的麦克风 | default=false | vb-cable=false");
    }

    #[test]
    fn 勾选包含机器名时不脱敏() {
        // 主机名受复选控制：勾选后原样保留；用户名不在复选范围，永远替换
        let body = "host=FISHAWDK user=unituser".to_string();
        let out = scrub_body(
            body,
            true,
            &["FISHAWDK".to_string()],
            &["unituser".to_string()],
        );
        assert_eq!(out, "host=FISHAWDK user=<redacted-host>");
    }

    #[test]
    fn 空候选不会污染文本() {
        // replace("", X) 会逐字符插 X；空候选必须在收集/变体阶段被丢弃
        let src = "abcdef 身份行".to_string();
        let out = scrub_body(src.clone(), false, &["".to_string()], &["".to_string()]);
        assert_eq!(out, src);
    }

    #[test]
    fn 脱敏幂等() {
        let body = "host=FISHAWDK-PC user=unituser dev=unituser 的麦克风".to_string();
        let once = scrub_body(body.clone(), false, &["FISHAWDK-PC".to_string()], &["unituser".to_string()]);
        let twice = scrub_body(once.clone(), false, &["FISHAWDK-PC".to_string()], &["unituser".to_string()]);
        assert_eq!(once, twice);
    }

    // ── T-A 追加加固：词边界 + 第三重用户名来源 ──────────

    #[test]
    fn 短账户名不打糊英文键名() {
        // 2~3 字符账户名是真实现实（本仓库开发者本人即 3 字符）：无边界子串替换
        // 会把 subtitle_max_lines / plugin_id 等报障要看的关键字打糊。
        let src = "  - plugin_id: x\n  - subtitle_max_lines: 3\n  - user_name: y\n  - min_app_version: 1.8".to_string();
        for cand in ["li", "max", "name", "id", "app", "min"] {
            let out = scrub_body(src.clone(), false, &[], &[cand.to_string()]);
            assert_eq!(out, src, "候选 {cand} 不得改动键名文本");
        }
        // 同一候选在真分隔符下仍须生效：证明不是一刀切失效造成的假绿
        assert_eq!(
            scrub_body("path=C:\\Users\\li\\x".into(), false, &[], &["li".to_string()]),
            "path=C:\\Users\\<redacted-host>\\x"
        );
    }

    #[test]
    fn 数据目录用户段提取与平台兼容() {
        assert_eq!(
            users_dir_segment(std::path::Path::new(
                "C:\\Users\\unituser\\AppData\\Roaming\\com.voiceassist.app"
            ))
            .as_deref(),
            Some("unituser")
        );
        assert_eq!(
            users_dir_segment(std::path::Path::new(
                "/home/unituser/.local/share/com.voiceassist.app"
            ))
            .as_deref(),
            Some("unituser")
        );
        // 段名大小写不敏感（Windows 习惯大写 Users）
        assert_eq!(
            users_dir_segment(std::path::Path::new("c:\\users\\UNITUSER\\x")).as_deref(),
            Some("UNITUSER")
        );
        // 非标准安装目录无用户段：返回 None 而非 panic
        assert_eq!(users_dir_segment(std::path::Path::new("D:\\App\\data")), None);
        assert_eq!(users_dir_segment(std::path::Path::new("")), None);
        // 路径以 Users 结尾（无下一段）也不得 panic
        assert_eq!(users_dir_segment(std::path::Path::new("C:\\Users")), None);
    }

    #[test]
    fn 环境变量缺失时由数据目录兜底() {
        // 不依赖真实 env：只要 data_dir 带用户段，候选集必须含它（env 双落空时的唯一防线）
        let probe = std::path::Path::new("C:\\Users\\probeuserxyz\\AppData\\Roaming\\com.voiceassist.app");
        let (_, users) = candidate_identities(probe);
        assert!(
            users.iter().any(|u| u == "probeuserxyz"),
            "data_dir 用户段必须成为用户名候选，实际: {users:?}"
        );
        // 且不因 env 是否存在而改变结果：同一 data_dir 能独立支撑脱敏
        let body = "data_dir: C:\\Users\\probeuserxyz\\AppData".to_string();
        let out = scrub_body(body, false, &[], &users);
        assert_eq!(out, "data_dir: C:\\Users\\<redacted-host>\\AppData");
    }

    // ── T-B：输入设备小节 ──────────────────────────────

    #[test]
    fn 音频节含输入设备标题且不panic() {
        // 真实环境枚举（CI/无设备环境也应正常返回空列表而非 panic）
        let out = std::panic::catch_unwind(render_audio)
            .expect("render_audio 不得 panic（分节兜底风格）");
        let has_title = out.lines().any(|l| l == "### 输入设备（录音）");
        assert!(has_title, "音频节必须含输入设备小节标题行");
    }
}
