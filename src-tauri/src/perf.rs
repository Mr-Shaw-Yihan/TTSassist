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

/// startup 暂存（T-G 分支 2 最小改动）：release 下前端首帧早于 logging::init
/// （实验实测早 ~234ms），emit 时落盘未就绪则暂存这里，init 完成后由
/// drain_startup_pending 补写。OnceLock 单值，不用队列/Vec/定时任务。
static PENDING_STARTUP: OnceLock<u64> = OnceLock::new();

/// 路由决策（纯函数，可测）：落盘就绪 → 立即发射；否则 → 暂存待 drain。
pub enum StartupRoute {
    Emit(u64),
    Pending(u64),
}

fn route_startup(ms: u64, file_enabled: bool, has_path: bool) -> StartupRoute {
    if file_enabled && has_path {
        StartupRoute::Emit(ms)
    } else {
        StartupRoute::Pending(ms)
    }
}

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
        match route_startup(
            ms,
            crate::logging::is_enabled(),
            crate::logging::log_file_path().is_some(),
        ) {
            StartupRoute::Emit(ms) => log_info!("{}", perf_line("startup", ms, &[])),
            StartupRoute::Pending(ms) => {
                let _ = PENDING_STARTUP.set(ms);
            }
        }
    }
}

/// logging::init 完成后调用（lib.rs setup 内）：把暂存的 startup 补写进日志。
/// 暂存为空（dev 下通常已直接发射）则无事发生。
pub fn drain_startup_pending() {
    if let Some(ms) = PENDING_STARTUP.get() {
        log_info!("{}", perf_line("startup", *ms, &[]));
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

    // ── T-G 分支 2：drain 前后行为 ─────────────────────

    #[test]
    fn startup路由_落盘未就绪时暂存不发射() {
        // 开关未开 / 日志路径未定（release 下前端首帧早于 logging::init 的真实场景），
        // 都走暂存而不是立即发射——drain 前 startup 行不应出现在日志流
        assert!(matches!(
            route_startup(659, false, false),
            StartupRoute::Pending(659)
        ));
        assert!(matches!(
            route_startup(659, true, false),
            StartupRoute::Pending(659)
        ));
        assert!(matches!(
            route_startup(659, false, true),
            StartupRoute::Pending(659)
        ));
    }

    #[test]
    fn startup路由_就绪时立即发射() {
        // 落盘开关开且路径已定 → 直接发射，无需 drain
        assert!(matches!(
            route_startup(659, true, true),
            StartupRoute::Emit(659)
        ));
    }
}
