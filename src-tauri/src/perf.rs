// 性能基线埋点（T2）：把「感觉有点慢」变成数字。
//
// 设计约束（任务书 §4.3，有意为之）：
//   - 只测不改：不引入 profiler、不改缓存策略、不做任何优化——优化是拿到数据后人的决定；
//   - 常驻 INFO 输出（不藏开关）：用户报障时需要这个数，开销仅两个 Instant；
//   - 只记长度不记内容（呼应 §3.4 隐私规则）；
//   - 「时长计算 + 格式化」抽成纯函数 perf_line，埋点逻辑可单测。

use std::sync::OnceLock;
use std::time::Instant;

/// 进程 `run()` 进入时刻（startup 指标起点），lib.rs 启动第一行写入。
pub static START: OnceLock<Instant> = OnceLock::new();

/// startup 是否已上报（只记首次，防多窗口/重渲染重复打点）
static STARTUP_LOGGED: OnceLock<()> = OnceLock::new();

/// 格式化一行性能日志：`[perf] <name>=<ms>ms k1=v1 k2=v2…`
/// fields 按传入顺序拼接，不做转义（字段值来自受控枚举/整数，无空格）。
pub fn perf_line(name: &str, ms: u64, fields: &[(&str, &str)]) -> String {
    let mut out = format!("[perf] {name}={ms}ms");
    for (k, v) in fields {
        out.push(' ');
        out.push_str(k);
        out.push('=');
        out.push_str(v);
    }
    out
}

/// 前端 main 窗口首帧回执（命令 perf_startup_done 调用）：
/// 从 run() 进入到前端首帧的总耗时，只上报第一次。
pub fn startup_done() {
    if STARTUP_LOGGED.set(()).is_err() {
        return; // 已上报过（quick_input/球窗等其它入口重复调用直接忽略）
    }
    if let Some(t0) = START.get() {
        let ms = t0.elapsed().as_millis() as u64;
        log_info!("{}", perf_line("startup", ms, &[]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 无字段时只name和毫秒() {
        assert_eq!(perf_line("startup", 1234, &[]), "[perf] startup=1234ms");
    }

    #[test]
    fn 字段按序拼接() {
        let out = perf_line("tts_first_audio", 842, &[("engine", "edge-tts"), ("cache", "miss"), ("len", "42")]);
        assert_eq!(out, "[perf] tts_first_audio=842ms engine=edge-tts cache=miss len=42");
    }

    #[test]
    fn 零与大数值正常() {
        assert_eq!(perf_line("x", 0, &[("n", "0")]), "[perf] x=0ms n=0");
        assert_eq!(perf_line("y", u64::MAX, &[]), "[perf] y=18446744073709551615ms");
    }
}
