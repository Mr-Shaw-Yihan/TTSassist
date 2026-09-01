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
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

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

/// 落盘总开关（“支持模式”）：默认关，仅写 stderr。
/// 由 `logging::init`（启动，读设置/env）与 `update_setting`（运行期切换）设置。
static FILE_ENABLED: AtomicBool = AtomicBool::new(false);
/// 日志文件路径（app_data_dir/logs/app.log），init 时写入一次。
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();
/// 串行化文件追加，避免多窗口/多线程交错写坏行。
static WRITE_LOCK: Mutex<()> = Mutex::new(());
/// 单文件上限，超过则在启动时轮转为 app.log.old（覆盖上一份），峰值约 4MB。
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// 初始化日志落盘：确定 logs 目录与开关状态。`enabled` = 设置 diagnostics_log_enabled 或 env VA_DIAG_LOG 命中。
pub fn init(log_dir: &PathBuf, enabled: bool) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = log_dir.join("app.log");
    // 启动裁剪：上次遗留的超大日志先滚存，避免无界增长
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = std::fs::rename(&path, log_dir.join("app.log.old"));
        }
    }
    let _ = LOG_FILE.set(path);
    FILE_ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        // 写一行会话分隔，方便对齐用户报障时间点
        emit(
            Level::Info,
            format_args!("── 诊断日志已开启（本文件仅存本地，不含你合成的文本） ──"),
        );
    }
}

/// 运行期切换落盘开关（update_setting 副作用调用）。
pub fn set_enabled(on: bool) {
    FILE_ENABLED.store(on, Ordering::Relaxed);
}

/// 当前是否落盘（供 UI/命令查询）。
pub fn is_enabled() -> bool {
    FILE_ENABLED.load(Ordering::Relaxed)
}

/// 日志文件路径（未初始化返回 None），供“打开位置”类交互用。
pub fn log_file_path() -> Option<PathBuf> {
    LOG_FILE.get().cloned()
}

/// 实际输出一行：始终写 stderr；开启支持模式时再追加一份到日志文件（best-effort）。
pub fn emit(level: Level, args: std::fmt::Arguments) {
    let ts = Local::now().format("%H:%M:%S%.3f");
    let line = format!("[{ts} {}] {}", level.tag(), args);
    eprintln!("{line}");
    if FILE_ENABLED.load(Ordering::Relaxed) {
        if let Some(path) = LOG_FILE.get() {
            if let Ok(_g) = WRITE_LOCK.lock() {
                if let Ok(mut f) =
                    std::fs::OpenOptions::new().create(true).append(true).open(path)
                {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }
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
