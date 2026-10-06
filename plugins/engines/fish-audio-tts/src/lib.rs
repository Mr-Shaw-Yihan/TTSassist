// Fish Audio TTS 插件 — 调用 api.fish.audio 云端语音合成（S2.1 系列）。
//
// 环境变量（由宿主通用插件配置机制按 manifest.config 声明注入）：
//   FISH_AUDIO_API_KEY        必填，fish.audio → Settings → API Keys 创建
//   FISH_AUDIO_MODEL          可选，默认 s2.1-pro-free（免费）；可选 s2.1-pro / s2-pro / s1
//   FISH_AUDIO_LATENCY        可选，normal（默认）/ balanced / low
//   FISH_AUDIO_VOICE_OVERRIDE 可选，音色模型 ID（reference_id），填写后覆盖下拉选择的音色
//   FISH_AUDIO_PROXY          可选，如 http://127.0.0.1:7897；不填则依次探测
//                             环境变量代理 → Windows 系统代理，都无则直连
//
// 输出：MP3 44.1kHz 128kbps 单声道（api.fish.audio 默认规格）。
// 音色 = Fish Audio 社区模型库的 reference_id；不选音色（NULL/"default"）用官方默认音色。

use serde::Serialize;
use std::io::Read;
use std::time::Duration;

plugin_api::va_tts_plugin! {
    id: "fish-audio-tts",
    name: "Fish Audio TTS",
    version: "0.1.0",
    audio_format: "mp3",
    voices: voices_list,
    synthesize: synthesize,
}

/// API 端点前缀
const BASE_URL: &str = "https://api.fish.audio";
/// 默认模型：s2.1-pro-free 为官方免费档（$0.00），无余额账户也可用
const DEFAULT_MODEL: &str = "s2.1-pro-free";
/// 合法 latency 档位（实测 normal/balanced/low 三值均返回 200）
const LATENCY_CHOICES: [&str; 3] = ["normal", "balanced", "low"];
/// 单次 HTTP 请求整体超时。宿主默认合成超时 60s，留出余量让插件先报出可读错误
const REQUEST_TIMEOUT: Duration = Duration::from_secs(50);

/// 音色下拉表：default + 13 个实测可用的社区音色（2026-10-06 用真实 Key 逐个验证）。
/// 挑选原则：中立风格（新闻/叙事/温柔/纪录片），不收名人克隆与梗音色。
const VOICES_JSON: &str = r#"[
{"id":"default","label":"默认音色（Fish Audio 官方）"},
{"id":"faccba1a8ac54016bcfc02761285e67f","label":"温柔动听女声"},
{"id":"57eab548c7ed4ddc974c4c153cb015b2","label":"女主播（新闻女声）"},
{"id":"59cb5986671546eaa6ca8ae6f29f6d22","label":"央视配音（新闻男声）"},
{"id":"2926cb350f1a426d800bf8c360c3cb94","label":"新闻男声"},
{"id":"23632847285a487d8e0c6ae5bc593c71","label":"温柔叙事女声"},
{"id":"32ea6ed2f793439da52685ac910534ac","label":"温柔女教师"},
{"id":"5c353fdb312f4888836a9a5680099ef0","label":"女大学生（自然女声）"},
{"id":"7f92f8afb8ec43bf81429cc1c9199cb1","label":"AD学姐（活泼女声）"},
{"id":"d99547e2dad64ce0aa085319a3c9cc56","label":"温柔男声（讲故事）"},
{"id":"7d4cc998f68c413ba5605d892d7acc87","label":"纪录片男声"},
{"id":"dd43b30d04d9446a94ebe41f301229b5","label":"宣传片男声（大气浑厚）"},
{"id":"933563129e564b19a115bedd57b7406a","label":"Sarah（英文女声）"},
{"id":"802e3bc2b27e49c2995d23ef70e6ac89","label":"Energetic Male（英文男声）"}
]"#;

/// 动态音色表函数（宿主每次查询都会调用，必须廉价且不 panic）
fn voices_list() -> Vec<plugin_api::VoiceItem> {
    serde_json::from_str(VOICES_JSON).unwrap_or_default()
}

// ── 请求构造 ──────────────────────────────────────────────

/// POST /v1/tts 请求体。字段名按官方 OpenAPI 为 snake_case；
/// reference_id 缺省时服务端用官方默认音色，故 None 时直接省略该键。
#[derive(Serialize)]
struct TtsRequest<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_id: Option<String>,
    format: &'a str,
    latency: &'a str,
}

/// 音色 → reference_id 的纯函数核心（环境变量覆盖由调用方读好后传入）。
/// 规则：下拉未选/选了 default → None（官方默认音色）；配置了覆盖值 → 覆盖。
fn resolve_reference_id_with(voice: Option<&str>, override_id: Option<&str>) -> Option<String> {
    if let Some(id) = override_id {
        let id = id.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    match voice {
        None | Some("") | Some("default") => None,
        Some(v) => Some(v.to_string()),
    }
}

/// latency 环境变量解析：空值用默认档；非法值直接报错（不静默兜底，
/// 与本仓库「非法入参一律报错」的口径一致，避免用户配错却听不出原因）
fn resolve_latency_with(raw: Option<&str>) -> Result<&'static str, String> {
    match raw.map(str::trim) {
        None | Some("") => Ok("normal"),
        Some(v) if LATENCY_CHOICES.contains(&v) => {
            // 约束保证返回值与入参同值且为 'static（LATENCY_CHOICES 内常量）
            Ok(LATENCY_CHOICES.iter().find(|c| **c == v).copied().unwrap())
        }
        Some(v) => Err(format!(
            "FISH_AUDIO_LATENCY 取值非法：{v}（合法值：normal / balanced / low）"
        )),
    }
}

// ── 合成主流程 ────────────────────────────────────────────

/// 文本 → MP3 字节（voice 为 None 或 "default" 用官方默认音色）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("合成文本为空".to_string());
    }
    let api_key = std::env::var("FISH_AUDIO_API_KEY").ok().filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            "未设置 Fish Audio API Key，请在设置页的「API 密钥」卡中填写。\n\
             获取地址：https://fish.audio （登录后 Settings → API Keys）"
                .to_string()
        })?;

    let model = std::env::var("FISH_AUDIO_MODEL").ok().map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let latency_raw = std::env::var("FISH_AUDIO_LATENCY").ok();
    let latency = resolve_latency_with(latency_raw.as_deref())?;
    let override_id = std::env::var("FISH_AUDIO_VOICE_OVERRIDE").ok();
    let req = TtsRequest {
        text,
        reference_id: resolve_reference_id_with(voice, override_id.as_deref()),
        format: "mp3",
        latency,
    };

    let agent = build_agent();
    let resp = agent
        .post(&format!("{BASE_URL}/v1/tts"))
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .set("model", &model)
        .timeout(REQUEST_TIMEOUT)
        .send_json(&req);

    match resp {
        Ok(r) => {
            // ureq 2.x 无 into_bytes，读 reader 收全量音频（分块响应统一缓冲）
            let mut reader = r.into_reader();
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Fish Audio 音频流读取失败: {e}"))?;
            if bytes.is_empty() {
                return Err("Fish Audio 返回空音频".to_string());
            }
            Ok(bytes)
        }
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            Err(friendly_error(code, &body))
        }
        Err(e) => Err(format!("Fish Audio 网络请求失败: {e}（若网络需代理，请在配置中填写代理地址）")),
    }
}

/// 非 2xx 响应 → 面向用户的中文错误（错误体统一为 {"status":..,"message":..}，
/// 422 参数校验是错误对象数组；均实测确认）
fn friendly_error(code: u16, body: &str) -> String {
    let api_msg = parse_error_body(body);
    match code {
        401 => format!(
            "Fish Audio API Key 无效或已过期（401{}），请检查设置页的 API 密钥",
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
        402 => format!(
            "Fish Audio 账户余额不足（402），当前模型需要付费；可在配置中把模型切换为免费档 s2.1-pro-free"
        ),
        429 => format!(
            "Fish Audio 请求过于频繁（429{}），请稍后重试",
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
        _ => format!(
            "Fish Audio 合成失败（HTTP {}{}）",
            code,
            api_msg.map(|m| format!("：{m}")).unwrap_or_default()
        ),
    }
}

/// 从错误响应体提取 message（对象 {"status","message"} 或数组 [{loc,msg,...}]）
fn parse_error_body(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v {
        serde_json::Value::Object(map) => map
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        serde_json::Value::Array(items) => {
            let msgs: Vec<String> = items
                .iter()
                .filter_map(|it| it.get("msg").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .collect();
            if msgs.is_empty() { None } else { Some(msgs.join("；")) }
        }
        _ => None,
    }
}

// ── 代理探测（api.fish.audio 国内直连常超时，代理是刚需）──
//
// 优先级：FISH_AUDIO_PROXY 显式配置 → 环境变量（HTTPS_PROXY/ALL_PROXY 等，
// 约定俗成）→ Windows 系统代理（注册表 ProxyEnable/ProxyServer）→ 直连。

fn build_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new();
    if let Some(p) = resolve_proxy() {
        if let Ok(proxy) = ureq::Proxy::new(&p) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

/// 代理解析纯函数核心（各来源按优先级取第一个非空值；纯空白视同未配置）
fn pick_proxy<'a>(
    explicit: Option<&'a str>,
    env_https: Option<&'a str>,
    env_all: Option<&'a str>,
    system: Option<String>,
) -> Option<String> {
    let from_str = |s: Option<&'a str>| -> Option<String> {
        s.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(normalize_proxy_url)
    };
    from_str(explicit)
        .or_else(|| from_str(env_https))
        .or_else(|| from_str(env_all))
        .or(system)
}

/// 实际代理解析：显式配置 → 环境变量 → Windows 系统代理
fn resolve_proxy() -> Option<String> {
    let explicit = std::env::var("FISH_AUDIO_PROXY").ok().filter(|s| !s.trim().is_empty());
    let env_https = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    let env_all = std::env::var("ALL_PROXY")
        .or_else(|_| std::env::var("all_proxy"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    pick_proxy(
        explicit.as_deref(),
        env_https.as_deref(),
        env_all.as_deref(),
        windows_system_proxy(),
    )
}

/// 补全代理 URL 的 scheme（注册表/环境变量常见 "127.0.0.1:7897" 裸地址）
fn normalize_proxy_url(raw: &str) -> String {
    let s = raw.trim();
    if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// 读 Windows 系统代理（HKCU Internet Settings），非 Windows 一律 None
#[cfg(windows)]
fn windows_system_proxy() -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    parse_windows_proxy_server(&server)
}

#[cfg(not(windows))]
fn windows_system_proxy() -> Option<String> {
    None
}

/// 解析注册表 ProxyServer 的三种实测取值形态：
///   "127.0.0.1:7897"                          → 全协议同一地址
///   "127.0.0.1:7897;localhost:7897"（罕见）    → 取第一段
///   "http=…;https=…;ftp=…"（分协议）           → 优先 https，其次 http
fn parse_windows_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        let mut https = None;
        let mut http = None;
        for part in server.split(';') {
            let part = part.trim();
            if let Some((scheme, addr)) = part.split_once('=') {
                match scheme.trim().to_ascii_lowercase().as_str() {
                    "https" => https = Some(addr.trim().to_string()),
                    "http" => http = Some(addr.trim().to_string()),
                    _ => {}
                }
            }
        }
        https.or(http).map(|addr| normalize_proxy_url(&addr))
    } else {
        // 理论上只应有单段，防御性取第一段
        let first = server.split(';').next().unwrap_or(server).trim();
        if first.is_empty() { None } else { Some(normalize_proxy_url(first)) }
    }
}

// ── 测试 ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 音色表是合法json且条目齐全() {
        let v: Vec<serde_json::Value> = serde_json::from_str(VOICES_JSON).unwrap();
        assert!(v.len() >= 14);
        assert_eq!(v[0]["id"], "default");
        // 除 default 外全部是 32 位十六进制的 Fish Audio 模型 ID
        for item in &v[1..] {
            let id = item["id"].as_str().unwrap();
            assert_eq!(id.len(), 32, "音色 id 长度应为 32：{id}");
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "音色 id 应为十六进制：{id}");
        }
        // 每条都有非空 label
        for item in &v {
            assert!(!item["label"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn 请求体_默认音色时省略reference_id() {
        let req = TtsRequest { text: "测试", reference_id: None, format: "mp3", latency: "normal" };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("reference_id").is_none(), "默认音色不应携带 reference_id 键");
        assert_eq!(json["text"], "测试");
        assert_eq!(json["format"], "mp3");
        assert_eq!(json["latency"], "normal");
    }

    #[test]
    fn 请求体_带音色时携带reference_id() {
        let req = TtsRequest {
            text: "测试",
            reference_id: Some("abc".into()),
            format: "mp3",
            latency: "low",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["reference_id"], "abc");
    }

    #[test]
    fn 音色解析_未选与default都用官方默认() {
        assert_eq!(resolve_reference_id_with(None, None), None);
        assert_eq!(resolve_reference_id_with(Some("default"), None), None);
        assert_eq!(resolve_reference_id_with(Some(""), None), None);
    }

    #[test]
    fn 音色解析_普通音色原样透传() {
        let id = "59cb5986671546eaa6ca8ae6f29f6d22";
        assert_eq!(resolve_reference_id_with(Some(id), None), Some(id.to_string()));
    }

    #[test]
    fn 音色解析_配置覆盖优先于下拉选择() {
        assert_eq!(
            resolve_reference_id_with(Some("59cb5986671546eaa6ca8ae6f29f6d22"), Some(" ffcf5a3e6d4b4b3db3ca8dfabbc3dd8f ")),
            Some("ffcf5a3e6d4b4b3db3ca8dfabbc3dd8f".to_string()),
            "覆盖值应去除首尾空白并优先生效"
        );
        // 覆盖值为空白时不算配置，回落到下拉选择
        assert_eq!(
            resolve_reference_id_with(Some("abc"), Some("   ")),
            Some("abc".to_string())
        );
        // 覆盖值为空白且未选音色 → 官方默认
        assert_eq!(resolve_reference_id_with(None, Some("")), None);
    }

    #[test]
    fn latency_空值用默认_非法值报错() {
        assert_eq!(resolve_latency_with(None).unwrap(), "normal");
        assert_eq!(resolve_latency_with(Some("")).unwrap(), "normal");
        assert_eq!(resolve_latency_with(Some(" balanced ")).unwrap(), "balanced");
        assert_eq!(resolve_latency_with(Some("low")).unwrap(), "low");
        assert!(resolve_latency_with(Some("fast")).is_err());
    }

    #[test]
    fn 错误体解析_对象与数组两种形态() {
        assert_eq!(
            parse_error_body(r#"{"status":401,"message":"Invalid Token"}"#).as_deref(),
            Some("Invalid Token")
        );
        let body = r#"[{"type":"literal_error","loc":["latency"],"msg":"bad value"}]"#;
        assert_eq!(parse_error_body(body).as_deref(), Some("bad value"));
        assert_eq!(parse_error_body("not json"), None);
    }

    #[test]
    fn 中文错误映射_常见状态码() {
        let e401 = friendly_error(401, r#"{"status":401,"message":"Invalid Token"}"#);
        assert!(e401.contains("API Key 无效"), "{e401}");
        assert!(e401.contains("Invalid Token"), "{e401}");
        assert!(friendly_error(402, "{}").contains("余额不足"));
        assert!(friendly_error(429, "{}").contains("频繁"));
        let e400 = friendly_error(400, r#"{"status":400,"message":"oops"}"#);
        assert!(e400.contains("HTTP 400") && e400.contains("oops"), "{e400}");
    }

    #[test]
    fn 代理解析_优先级与环境变量格式() {
        // 显式配置 > 环境变量 > 系统代理 > 直连
        assert_eq!(
            pick_proxy(Some("127.0.0.1:7897"), Some("http://a:1"), None, Some("http://s:1".into())).as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            pick_proxy(None, Some("http://a:1"), None, Some("http://s:1".into())).as_deref(),
            Some("http://a:1")
        );
        assert_eq!(
            pick_proxy(None, None, Some("socks5://b:2"), Some("http://s:1".into())).as_deref(),
            Some("socks5://b:2")
        );
        assert_eq!(
            pick_proxy(None, None, None, Some("http://s:1".into())).as_deref(),
            Some("http://s:1")
        );
        assert_eq!(pick_proxy(None, None, None, None), None);
        // 空串与纯空白视同未配置
        assert_eq!(pick_proxy(Some("  "), None, None, None), None);
    }

    #[test]
    fn 代理解析_注册表服务器三种形态() {
        // 形态一：全协议裸地址
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        // 形态二：分协议，https 优先
        assert_eq!(
            parse_windows_proxy_server("http=10.0.0.1:8080;https=10.0.0.2:8443;ftp=10.0.0.3:21").as_deref(),
            Some("http://10.0.0.2:8443")
        );
        // 形态二变体：只有 http 协议条目
        assert_eq!(
            parse_windows_proxy_server("http=10.0.0.1:8080").as_deref(),
            Some("http://10.0.0.1:8080")
        );
        // 形态三：多段裸地址取第一段
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897;localhost:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(parse_windows_proxy_server(""), None);
        assert_eq!(parse_windows_proxy_server("   "), None);
    }
}
