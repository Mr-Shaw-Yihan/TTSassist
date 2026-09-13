// 诊断包脱敏（T1 二次防线）：日志行写进诊断包前逐行过滤。
//
// 与 §3.4-A「采集层面不取用户文本」互补：日志尾部内容不在采集控制内，
// 必须在这里兜底。规则（有意保持字面、保守）：
//   1. 把 text="…" / "text":"…" / text: … / 内容= 四种形式后的可变长串
//      替换为 <redacted len=N>（N 为被抹掉的字符数）；
//   2. 任意 32+ 位十六进制串、长度 ≥20 的 base64 样串替换为 <redacted token>
//      （防 API Key / token 经日志泄漏）；
//   3. 每行硬截断到 200 字符，超出追加 …[truncated]。
// 纯字符串处理，不碰 IO；边界一律按 char 计，杜绝中文多字节切片 panic。

/// 单行最大保留字符数（超出截断并追加标记）
const MAX_LINE_CHARS: usize = 200;

/// 判断 c 是否属于 base64 字母表（含填充符 '='）。
/// 十六进制字符集是其子集，因此 32+ 位 hex 串必然落在 base64 扫描的 run 里。
fn is_b64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='
}

fn is_hex_char(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// 把连续同字符集的 run 中的 token 换成占位符：
/// 全 hex 且 ≥32 位 → hex 规则；否则 base64 样且 ≥20 位 → base64 规则。
fn redact_token_runs(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if is_b64_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_b64_char(chars[i]) {
                i += 1;
            }
            let run: Vec<char> = chars[start..i].to_vec();
            let all_hex = run.iter().all(|&c| is_hex_char(c));
            let len = run.len();
            if (all_hex && len >= 32) || len >= 20 {
                out.push_str("<redacted token>");
            } else {
                out.extend(run);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 匹配点的前一个字符不得为 ASCII 字母/数字/下划线（防止 text2= / fmt_text: 误伤）。
fn boundary_ok(chars: &[char], start: usize) -> bool {
    if start == 0 {
        return true;
    }
    let prev = chars[start - 1];
    !(prev.is_ascii_alphanumeric() || prev == '_')
}

/// 四种用户文本形态的脱敏。带引号的两种先替换（精确界定内容长度），
/// 再处理到行尾的两种（text: … / 内容= …）。
fn redact_text_assignments(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let quoted_openers: Vec<Vec<char>> = ["text=\"", "\"text\":\""]
        .iter()
        .map(|o| o.chars().collect())
        .collect();

    // 第一轮：text="…" / "text":"…" —— 从匹配点到闭合引号整段替换，一行内可多段
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let mut matched = false;
        for open in &quoted_openers {
            if boundary_ok(&chars, i) && starts_with(&chars, i, open) {
                let content_start = i + open.len();
                if let Some(close_rel) = find_sub(&chars[content_start..], &['"']) {
                    let content: String =
                        chars[content_start..content_start + close_rel].iter().collect();
                    out.extend(open.iter());
                    out.push_str(&format!("<redacted len={}>", content.chars().count()));
                    out.push('"'); // 闭合引号保留，维持日志行形态
                    i = content_start + close_rel + 1;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            out.push(chars[i]);
            i += 1;
        }
    }

    // 第二轮：text: … / 内容= … —— 首个边界匹配点起直到行尾全部替换（保守方向）
    for open in ["text:", "内容="] {
        let cur: Vec<char> = out.chars().collect();
        let open_c: Vec<char> = open.chars().collect();
        let mut found = None;
        for i in 0..cur.len() {
            if boundary_ok(&cur, i) && starts_with(&cur, i, &open_c) {
                found = Some(i);
                break;
            }
        }
        if let Some(pos) = found {
            let content: String = cur[pos + open_c.len()..].iter().collect();
            let lead = if open.ends_with(':') { " " } else { "" };
            out = format!(
                "{}{open}{lead}<redacted len={}>",
                cur[..pos].iter().collect::<String>(),
                content.chars().count(),
            );
        }
    }
    out
}

fn starts_with(chars: &[char], at: usize, pat: &[char]) -> bool {
    at + pat.len() <= chars.len() && chars[at..at + pat.len()] == *pat
}

fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// 按 char 计数截断到 200 字符，超出追加 …[truncated]。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}…[truncated]")
}

/// 单行脱敏入口：文本形态 → token → 硬截断。
pub fn redact_line(line: &str) -> String {
    let s = redact_text_assignments(line);
    let s = redact_token_runs(&s);
    truncate_chars(&s, MAX_LINE_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 正常日志行不动() {
        let line = "[14:23:01.123 INFO ] [remote] WS 服务已启动: 0.0.0.0:45271";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn 引号text被脱敏() {
        let out = redact_line("合成请求 text=\"救救我\" 已发出");
        assert_eq!(out, "合成请求 text=\"<redacted len=3>\" 已发出");
    }

    #[test]
    fn json形式text被脱敏() {
        let out = redact_line("{\"level\":\"info\",\"text\":\"救我\",\"ok\":true}");
        assert_eq!(out, "{\"level\":\"info\",\"text\":\"<redacted len=2>\",\"ok\":true}");
    }

    #[test]
    fn 裸text冒号与内容等号到行尾脱敏() {
        let out = redact_line("收到输入 text: 今天天气很好");
        assert_eq!(out, "收到输入 text: <redacted len=7>");
        let out2 = redact_line("用户内容=把这句话念出来");
        assert_eq!(out2, "用户内容=<redacted len=7>");
    }

    #[test]
    fn 超长行截断到200字符() {
        // 用 "ok " 重复填充（短 run 不会触发 token 吞噬），总长 600 字符
        let line = "ok ".repeat(200);
        let out = redact_line(&line);
        assert_eq!(out.chars().count(), 200 + "…[truncated]".chars().count());
        assert!(out.ends_with("…[truncated]"));
    }

    #[test]
    fn base64与hex样token被吞() {
        let out = redact_line("key=AbCdEfGhIjKlMnOpQrSt 请求已发");
        assert!(out.contains("<redacted token>"), "20位 base64 样串应被吞: {out}");
        assert!(!out.contains("AbCdEfGhIjKlMnOpQrSt"));
        let hex = "a".repeat(40);
        let out2 = redact_line(&format!("sha256={hex} ok"));
        assert!(out2.contains("<redacted token>"));
        assert!(!out2.contains(&hex));
    }

    #[test]
    fn 中文多字节截断不panic() {
        let line = "汉".repeat(400);
        let out = redact_line(&line); // 切点必然落在多字节序列中间
        assert_eq!(out.chars().take(200).count(), 200);
        assert!(out.ends_with("…[truncated]"));
        // 混合行：脱敏切点在中文内容里也不 panic
        let mixed = format!("text=\"{}\" 收尾", "文".repeat(300));
        let out2 = redact_line(&mixed);
        assert!(out2.starts_with("text=\"<redacted len=300>"));
    }

    #[test]
    fn 空行原样() {
        assert_eq!(redact_line(""), "");
    }

    #[test]
    fn 无匹配行原样() {
        let line = "普通一行日志，没有任何敏感形态 okay";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn crlf结尾不panic且保留内容() {
        let out = redact_line("[info] done=ok\r\n");
        assert!(out.starts_with("[info] done=ok"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn 短串与普通单词不受token规则影响() {
        // 15 位 base64 样串 < 20 不吞；带分隔的短 hex 不吞
        let line = "id=AbCdEfGhIjKlMnO hash=a1b2c3 ok";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn 伪匹配边界不误伤() {
        // fmt_text: / text2= 不应触发脱敏（前缀边界检查）
        let line = "fmt_text: 保留 text2=保留";
        assert_eq!(redact_line(line), line);
    }
}
