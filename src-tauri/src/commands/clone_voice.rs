// 克隆音色样本导入/删除命令。
// 首版只支持一个克隆样本。

use std::path::Path;
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 导入克隆音色样本：复制本地文件到 voice_samples/ 下，
/// 并把样本路径与用户起的名字写进 settings。
#[tauri::command]
pub fn import_clone_voice(
    file_path: String,
    name: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let data_dir = &state.data_dir;
    let src = Path::new(&file_path);

    // 限制大小：MiMo 文档要求 ≤10MB
    let meta = std::fs::metadata(src).map_err(|e| format!("读取样本文件失败: {e}"))?;
    if meta.len() > 10 * 1024 * 1024 {
        return Err("样本文件过大，MiMo 限 10MB 以内".into());
    }

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "mp3".into());
    if !matches!(ext.as_str(), "mp3" | "wav") {
        return Err("仅支持 mp3 或 wav 格式样本".into());
    }

    let target_dir = data_dir.join("voice_samples");
    std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建样本目录失败: {e}"))?;

    // 若已存在旧样本，先移除文件
    {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        if !s.clone_voice_path.is_empty() {
            let old = data_dir.join(&s.clone_voice_path);
            let _ = std::fs::remove_file(&old);
        }
    }

    let rel_path = format!("voice_samples/clone.{ext}");
    let dest = data_dir.join(&rel_path);
    std::fs::copy(src, &dest).map_err(|e| format!("复制样本失败: {e}"))?;

    // 连续写两个键，第二次 update 返回的即最终状态
    crate::storage::settings::update_setting(data_dir, "clone_voice_path", serde_json::json!(rel_path))
        .map_err(|e| format!("{e}"))?;
    let new_settings = crate::storage::settings::update_setting(
        data_dir,
        "clone_voice_name",
        serde_json::json!(name.clone()),
    )
    .map_err(|e| format!("{e}"))?;
    {
        let mut guard = state
            .settings
            .write()
            .map_err(|e| format!("更新内存设置失败: {e}"))?;
        *guard = new_settings;
    }

    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(())
}

/// 删除克隆音色样本：清空两字段 + 删文件。
#[tauri::command]
pub fn remove_clone_voice(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let data_dir = &state.data_dir;

    let old_path = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        s.clone_voice_path.clone()
    };

    if !old_path.is_empty() {
        let _ = std::fs::remove_file(data_dir.join(&old_path));
    }

    crate::storage::settings::update_setting(data_dir, "clone_voice_path", serde_json::json!(""))
        .map_err(|e| format!("{e}"))?;
    let new_settings = crate::storage::settings::update_setting(
        data_dir,
        "clone_voice_name",
        serde_json::json!(""),
    )
    .map_err(|e| format!("{e}"))?;
    {
        let mut guard = state
            .settings
            .write()
            .map_err(|e| format!("更新内存设置失败: {e}"))?;
        *guard = new_settings;
    }

    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(())
}