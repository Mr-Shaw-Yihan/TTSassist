// 插件系统：把 .dll 动态库插件加载为可用的 TTS 引擎。
//
// 组成：
// - manifest：插件清单（manifest.json）结构与校验
// - registry：已安装插件注册表（registry.json）
// - loader：libloading 加载 dll + SHA-256 校验 + TTSEngine 包装
// - manager：PluginManager（Tauri State），启动时加载全部插件
//
// 总体设计见 doc/插件系统规划.md，本阶段实现见 doc/开发记录.md 阶段 16。

pub mod loader;
pub mod manager;
pub mod manifest;
pub mod registry;

pub use loader::{LoadedPlugin, PluginEngine};
pub use manager::{PluginInfo, PluginManager};
pub use manifest::PluginManifest;

use thiserror::Error;

/// 插件子系统的错误类型
#[derive(Debug, Error)]
pub enum PluginError {
    /// 插件目录/清单/动态库缺失
    #[error("插件文件缺失: {0}")]
    NotFound(String),

    /// manifest.json 解析失败
    #[error("清单错误: {0}")]
    Manifest(String),

    /// 平台/类型/版本/id 不满足加载条件
    #[error("插件不可用: {0}")]
    Unsupported(String),

    /// SHA-256 校验不通过（dll 可能被篡改）
    #[error("SHA-256 校验失败（期望 {expected}，实际 {actual}），dll 可能已损坏或被篡改")]
    Checksum { expected: String, actual: String },

    /// libloading 加载失败
    #[error("动态库加载失败: {0}")]
    DlOpen(String),

    /// dll 缺少约定的导出符号
    #[error("导出符号错误: {0}")]
    Symbol(String),

    /// 合成调用失败（插件返回的错误消息）
    #[error("{0}")]
    Synthesize(String),

    /// 文件 IO 错误
    #[error("IO 错误: {0}")]
    Io(String),
}
