// 插件管理命令（第 3 步 UI 会用到，本步先提供后端能力）。

use tauri::State;
use crate::plugins::{PluginInfo, PluginManager};

/// 列出已安装插件（含加载状态、失败原因、音色表）
#[tauri::command]
pub fn list_plugins(plugins: State<'_, PluginManager>) -> Vec<PluginInfo> {
    plugins.list()
}
