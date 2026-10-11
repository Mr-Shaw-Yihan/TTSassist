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
pub mod floating_ball;
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
/// - `data_dir`: 内容根（消息、音频、样本、日志、插件重数据）。**当前默认仍等于配置根**，
///   翻默认见存储计划 Task 4 Step 6；新代码请从 `crate::storage::paths::StoragePaths` 取路径
/// - `settings`: 缓存一份在内存中（读多写少），更新时同时写文件 + 改内存
pub struct AppState {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf, settings: Settings) -> Self {
        Self {
            config_dir,
            data_dir,
            settings: RwLock::new(settings),
        }
    }
}
