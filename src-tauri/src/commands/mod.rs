// Tauri 命令层——前后端桥接。
//
// 每个命令对应前端的一个操作（发消息/删消息/读设置等），
// 负责: 接收前端参数 → 调存储层/TTS引擎 → 广播事件 → 返回序列化结果。
//
// AppState 通过 Tauri 的 manage() 注入，命令通过 State<'_, AppState> 访问。

pub mod audio;
pub mod clone_voice;
pub mod favorite;
pub mod message;
pub mod mic;
pub mod minimax_clone;
pub mod plugins;
pub mod remote;
pub mod settings;
pub mod tts;
pub mod update;
pub mod vbcable;

use std::path::PathBuf;
use std::sync::RwLock;
use crate::storage::types::Settings;

/// 多命令共享的全局状态。
///
/// - `data_dir`: Tauri app_data_dir，不可变，启动时初始化
/// - `settings`: 缓存一份在内存中（读多写少），更新时同时写文件 + 改内存
pub struct AppState {
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, settings: Settings) -> Self {
        Self {
            data_dir,
            settings: RwLock::new(settings),
        }
    }
}
