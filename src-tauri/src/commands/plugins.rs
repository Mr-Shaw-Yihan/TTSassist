// 插件管理命令。

use tauri::State;
use crate::plugins::{PluginInfo, PluginManager};

/// 列出已安装插件（含加载状态、失败原因、音色表）
#[tauri::command]
pub fn list_plugins(plugins: State<'_, PluginManager>) -> Vec<PluginInfo> {
    plugins.list()
}

/// 卸载插件：注册表移除 + 删目录。已加载的 dll 常驻到进程退出，
/// 返回文案告知用户是否需要重启。
#[tauri::command]
pub fn uninstall_plugin(
    id: String,
    plugins: State<'_, PluginManager>,
) -> Result<String, String> {
    plugins.uninstall(&id).map_err(|e| e.to_string())
}
