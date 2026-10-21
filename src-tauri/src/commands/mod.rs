// Tauri 命令层——前后端桥接。
//
// 每个命令对应前端的一个操作（发消息/删消息/读设置等），
// 负责: 接收前端参数 → 调存储层/TTS引擎 → 广播事件 → 返回序列化结果。
//
// AppState 通过 Tauri 的 manage() 注入，命令通过 State<'_, AppState> 访问。

pub mod audio;
pub mod clone_voice;
pub mod diag;
pub mod favorite;
pub mod message;
pub mod mic;
pub mod minimax_clone;
pub mod perf;
pub mod plugins;
pub mod remote;
pub mod settings;
pub mod storage;
pub mod subtitle;
pub mod tts;
pub mod tts_stream;
pub mod update;
pub mod vbcable;

use std::path::PathBuf;
use std::sync::RwLock;
use crate::storage::types::Settings;

/// 多命令共享的全局状态。
///
/// - `config_dir`: 配置根（%APPDATA%\com.voiceassist.app），放 settings / 遥控配对 / 存储指针
/// - `content_dir`: 内容根（消息、音频、样本、日志、插件重数据）。默认 = 配置根
///   （`%APPDATA%\com.voiceassist.app`），用户自选时由 `VA_STORAGE_DIR` > storage.json 指针
///   决定；`<exe>\data` 仅为 1.9.0 测试版历史来源。新代码请从 `StoragePaths` 取路径
/// - `settings`: 缓存一份在内存中（读多写少），更新时同时写文件 + 改内存
pub struct AppState {
    pub config_dir: PathBuf,
    pub content_dir: PathBuf,
    pub settings: RwLock<Settings>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, content_dir: PathBuf, settings: Settings) -> Self {
        Self {
            config_dir,
            content_dir,
            settings: RwLock::new(settings),
        }
    }
}
