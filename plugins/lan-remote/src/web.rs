// Web 遥控：自托管单文件页面（remote_page.html 编译进 dll）+ 图标常量。
//
// 页面经 server.rs 的同端口分流在 http://<PC-IP>:45271 直接 serve——
// 页面与 WS 同源，浏览器无 Mixed Content 限制，iOS 等无 App 设备扫码即用。
// 图标形状与 remote-app/assets/icons/（gen_icons.py 产物）保持一致；
// 改图标形状时须同步此处（单色 mask 版，斜杠以黑色挖空）。

/// 页面模板（占位符 __MIC_ON__/__MIC_OFF__/__PLAY_LAST__/__SEND__ 运行时替换）
pub static REMOTE_PAGE: &str = include_str!("remote_page.html");

// 实心图标（单色，供 CSS mask 用；斜杠用黑色挖空）
pub const MIC_ON: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'><g fill='black'><rect x='15.5' y='5' width='17' height='20' rx='8.5'/><path d='M13 23 A11 11 0 0 0 35 23 L31 23 A7 7 0 0 1 17 23 Z'/><rect x='21.8' y='34' width='4.4' height='5.2' rx='1'/><rect x='15.5' y='40.4' width='17' height='3.4' rx='1.7'/></g></svg>";

pub const MIC_OFF: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'><defs><mask id='m'><rect width='48' height='48' fill='white'/><path d='M9 7 L41 41' stroke='black' stroke-width='4.6' stroke-linecap='round'/></mask></defs><g mask='url(#m)' fill='black'><rect x='15.5' y='5' width='17' height='20' rx='8.5'/><path d='M13 23 A11 11 0 0 0 35 23 L31 23 A7 7 0 0 1 17 23 Z'/><rect x='21.8' y='34' width='4.4' height='5.2' rx='1'/><rect x='15.5' y='40.4' width='17' height='3.4' rx='1.7'/></g></svg>";

pub const PLAY_LAST: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'><g fill='black'><rect x='13' y='10.5' width='4.6' height='27' rx='1'/><path d='M37 11.8 V36.2 c0 1.35 -1.52 2.14 -2.62 1.36 L18.1 26.1 c-1.47 -1.04 -1.47 -3.22 0 -4.26 L34.38 10.44 c1.1 -0.78 2.62 0.01 2.62 1.36 Z'/></g></svg>";

pub const SEND: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'><path fill='black' d='M42.7 5.6 L4.3 21.2 c-1.7 .7 -1.6 3.1 .2 3.7 l10.6 3.4 3.9 12.4 c.5 1.7 2.8 2 3.8 .5 l5.3 -7.6 9.6 7.1 c1.3 1 3.2 .3 3.6 -1.3 l4.3 -30.5 c.3 -1.9 -1.7 -3.4 -3.4 -2.6 Z'/></svg>";

/// 极简 percent-encode（data URL 内的 SVG：编码保留字符以外的字节）
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            // 单引号必须编码：外层 CSS url('...') 用单引号包裹，裸 ' 会提前闭合
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!'
            | b'~' | b'*' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn icon_url(svg: &str) -> String {
    format!("data:image/svg+xml,{}", encode_uri_component(svg))
}

/// 组装最终页面（图标占位符 → url-encoded data URL）
pub fn build_page() -> String {
    REMOTE_PAGE
        .replace("__MIC_ON__", &encode_uri_component(MIC_ON))
        .replace("__MIC_OFF__", &encode_uri_component(MIC_OFF))
        .replace("__PLAY_LAST__", &encode_uri_component(PLAY_LAST))
        .replace("__SEND__", &encode_uri_component(SEND))
}

/// HTTP 路由：GET / → 遥控页面；其余 404。
/// 返回 (状态行, Content-Type, body)。
pub fn http_response(path: &str) -> (&'static str, &'static str, Vec<u8>) {
    match path {
        "/" | "/index.html" | "/remote" => {
            ("200 OK", "text/html; charset=utf-8", build_page().into_bytes())
        }
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"VoiceAssist remote: use http://<host>:45271/".to_vec(),
        ),
    }
}
