// 轻量结构化日志：为控制台输出统一加「时间 + 级别」前缀，便于开发/终端运行时排障。
//
// 设计约束（有意为之，勿擅自扩展）：
//   - 零新增依赖：复用工程已有的 chrono。
//   - 仅写标准错误流，不落盘：打包成 GUI（无控制台）后 stderr 会被系统丢弃，
//     本模块的价值是「终端/开发运行」；生产可见日志需文件落盘，属独立决策，暂未做。
//   - 不改变任何控制流：仅替换原有 eprintln! 的输出形态。
//
// 用法：log_info!/log_warn!/log_error!("...{var}", extra) —— 参数与 println! 完全一致。
// 迁移策略：核心路径（插件加载/启动/快捷键）先行，其余按需渐进（渐进迁移，非一次性大改）。

use chrono::Local;

/// 日志级别。ERROR 表示某功能失败但通常已降级处理；WARN 表示可恢复异常/提示；
/// INFO 表示正常流程节点（加载完成、注入、迁移等）。
#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// 实际输出一行：`[HH:MM:SS.mmm LEVEL] message` 到 stderr。
pub fn emit(level: Level, args: std::fmt::Arguments) {
    let ts = Local::now().format("%H:%M:%S%.3f");
    eprintln!("[{ts} {}] {}", level.tag(), args);
}

macro_rules! log_info {
    ($($arg:tt)*) => { $crate::logging::emit($crate::logging::Level::Info, ::core::format_args!($($arg)*)) };
}

macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::logging::emit($crate::logging::Level::Warn, ::core::format_args!($($arg)*)) };
}

macro_rules! log_error {
    ($($arg:tt)*) => { $crate::logging::emit($crate::logging::Level::Error, ::core::format_args!($($arg)*)) };
}
