// 设置相关命令：读设置、更新单键。

use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::plugins::PluginManager;
use crate::storage::types::Settings;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 读取当前设置（全部字段）。
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state
        .settings
        .read()
        .map(|s| s.clone())
        .map_err(|e| format!("读取设置失败: {e}"))
}

/// 更新单个设置键。
///
/// 同时写文件 + 更新内存中的 AppState（两者保持一致）。
/// value 为 serde_json::Value 以支持多种字段类型（字符串/数字）。
#[tauri::command]
pub fn update_setting(
    key: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
    plugins: State<'_, PluginManager>,
    app: AppHandle,
) -> Result<Settings, String> {
    // settings.json 属配置 → 写配置根（存储契约）
    let config_dir = &state.config_dir;

    // 先写文件（确保持久化优先）
    let settings = crate::storage::settings::update_setting(config_dir, &key, value)
        .map_err(|e| format!("保存设置失败: {e}"))?;

    // 再更新内存（读写锁）
    let mut guard = state
        .settings
        .write()
        .map_err(|e| format!("更新内存设置失败: {e}"))?;
    *guard = settings.clone();

    // 遗留顶层键不再同步环境变量：API Key 统一经 manifest config 注入
    //（见 plugins::config::inject_fields 与 set_plugin_config 命令）

    // 插件配置变更：按 manifest 声明同步环境变量（通用机制，插件下次合成即生效）
    if key == "plugin_config" {
        plugins.inject_config_env(&settings.plugin_config);
    }

    // 诊断日志（支持模式）开关：运行期即时切换是否落盘
    if key == "diagnostics_log_enabled" {
        crate::logging::set_enabled(settings.diagnostics_log_enabled);
    }

    notify_changed(&app, EVENT_SETTINGS_CHANGED);

    Ok(settings)
}
