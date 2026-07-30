// 收藏相关命令：列表、新增、删除、导入外部音频。
//
// 收藏策略（用户拍板）：仅引用源消息音频，不拷贝文件。
// 依靠 storage::audio_gc 的无引用则删逻辑保护音频不被误删。
// 例外：导入外部音频时必须复制到 audio/ 下，否则源文件移动后失效。

use std::path::Path;
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::storage::types::{Favorite, gen_id, now_iso};
use crate::sync::{notify_changed, EVENT_FAVORITE_CHANGED};

/// 读取全部收藏。
#[tauri::command]
pub fn list_favorites(state: State<'_, AppState>) -> Vec<Favorite> {
    crate::storage::favorites::load_favorites(&state.data_dir)
}

/// 从消息添加收藏：通过 source_message_id 找到原消息音频路径，
/// 仅在收藏中引用同一条音频文件（不拷贝）。
/// note 为必填备注（非空校验在 storage 层），source_message_id 必须对应一条消息。
#[tauri::command]
pub fn add_favorite(
    source_message_id: String,
    note: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Favorite, String> {
    let data_dir = &state.data_dir;

    // 查找源消息获取 audio_path
    let messages = crate::storage::messages::load_messages(data_dir);
    let msg = messages
        .iter()
        .find(|m| m.id == source_message_id)
        .ok_or_else(|| "找不到该消息，无法收藏".to_string())?;

    let fav = Favorite {
        id: gen_id("f"),
        source_message_id: Some(source_message_id),
        note,
        audio_path: msg.audio_path.clone(),
        created_at: now_iso(),
        hotkey: None,
    };

    let result = fav.clone();
    crate::storage::favorites::add_favorite(data_dir, fav)
        .map_err(|e| format!("{e}"))?;

    notify_changed(&app, EVENT_FAVORITE_CHANGED);

    Ok(result)
}

/// 删除一条收藏（存储层自动处理：收藏删除后音频无引用则清除）。
#[tauri::command]
pub fn delete_favorite(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let data_dir = &state.data_dir;
    let result = crate::storage::favorites::delete_favorite(data_dir, &id)
        .map_err(|e| format!("删除收藏失败: {e}"))?;
    if result {
        notify_changed(&app, EVENT_FAVORITE_CHANGED);
    }
    Ok(result)
}

/// 导入外部音频文件为收藏：
/// 把用户选择的绝对路径文件复制到 audio/ 下，source_message_id = None。
///
/// `file_path` 是用户通过前端 dialog 选出的绝对路径（如 D:\xxx\clap.mp3）。
#[tauri::command]
pub fn import_favorite(
    file_path: String,
    note: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Favorite, String> {
    let data_dir = &state.data_dir;
    let src = Path::new(&file_path);

    // 取扩展名（小写），无扩展名默认 mp3
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "mp3".into());

    // 生成收藏 id 与目标文件名
    let id = gen_id("f");
    let rel_path = format!("audio/{id}.{ext}");
    let dest = data_dir.join(&rel_path);

    // 确保目录存在
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败: {e}"))?;
    }
    // 复制文件（非移动，原文件保持）
    std::fs::copy(src, &dest)
        .map_err(|e| format!("复制音频失败: {e}"))?;

    let fav = Favorite {
        id,
        source_message_id: None,
        note,
        audio_path: rel_path,
        created_at: now_iso(),
        hotkey: None,
    };

    let result = fav.clone();
    crate::storage::favorites::add_favorite(data_dir, fav)
        .map_err(|e| format!("{e}"))?;

    notify_changed(&app, EVENT_FAVORITE_CHANGED);

    Ok(result)
}

/// 为某收藏设置快捷键（含冲突检测），设置后刷新全局快捷键注册。
#[tauri::command]
pub fn set_favorite_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    hotkey: String,
) -> Result<Vec<Favorite>, String> {
    let hotkey = hotkey.trim().to_string();
    if hotkey.is_empty() {
        return Err("快捷键不能为空".into());
    }

    // 冲突检测 1：与浮窗呼出快捷键相同
    let floating_hotkey = state
        .settings
        .read()
        .map(|s| s.hotkey_show_window.clone())
        .unwrap_or_default();
    if hotkey == floating_hotkey {
        return Err("与「呼出浮窗」快捷键冲突，请换一个".into());
    }

    // 冲突检测 2：与其它收藏的快捷键相同
    let favorites = crate::storage::favorites::load_favorites(&state.data_dir);
    for fav in &favorites {
        if fav.id != id && fav.hotkey.as_deref() == Some(hotkey.as_str()) {
            return Err(format!("与收藏「{}」的快捷键冲突，请换一个", fav.note));
        }
    }

    // 写入快捷键
    let updated = crate::storage::favorites::set_favorite_hotkey(&state.data_dir, &id, Some(hotkey))
        .map_err(|e| format!("{e}"))?;

    // 刷新全局快捷键注册
    crate::hotkey::refresh_favorite_hotkeys(&app, &updated)?;

    notify_changed(&app, EVENT_FAVORITE_CHANGED);

    Ok(updated)
}

/// 移除某收藏的快捷键，刷新注册。
#[tauri::command]
pub fn remove_favorite_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<Favorite>, String> {
    let updated = crate::storage::favorites::set_favorite_hotkey(&state.data_dir, &id, None)
        .map_err(|e| format!("{e}"))?;

    crate::hotkey::refresh_favorite_hotkeys(&app, &updated)?;

    notify_changed(&app, EVENT_FAVORITE_CHANGED);

    Ok(updated)
}
