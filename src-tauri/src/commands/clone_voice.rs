// 克隆音色样本导入/删除命令：宿主只管「一份通用样本文件」的存储与告知，
// 哪个引擎用它、支持哪些格式、多大上限，全部由插件自己判定（基座不认引擎身份）。
// 首版只支持一个克隆样本。

use std::path::Path;
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 样本绝对路径经通用环境变量 `VA_CLONE_SAMPLE` 告知插件（与 API Key 同一路数：
/// 插件运行期 std::env::var 读取，改完即生效，无需重启）。
pub(crate) fn announce_clone_sample(abs_path: &Path) {
    std::env::set_var("VA_CLONE_SAMPLE", abs_path);
}

/// 样本删除后撤销告知：留一个指向已消失文件的 env 会让插件报「样本文件不存在」，
/// 不如直接 remove，让插件据此给出「请先导入样本」的文案。
pub(crate) fn clear_clone_sample() {
    std::env::remove_var("VA_CLONE_SAMPLE");
}

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
    // 样本文件属内容（data_dir），而两次设置回写属配置（config_dir）——
    // 两者在翻默认前是同一个目录，但归属必须现在就分清
    let config_dir = &state.config_dir;
    let src = Path::new(&file_path);

    // 通用防呆上限 20MB：宿主不检查引擎各自的限制（如某云端引擎要求 ≤10MB），
    // 精确校验由插件在合成时给出中文原因，避免把引擎知识写进基座。
    let meta = std::fs::metadata(src).map_err(|e| format!("读取样本文件失败: {e}"))?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("样本文件过大（上限 20MB），请截取更短的音频".into());
    }

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "mp3".into());
    // 常见音频容器一律放行：能不能真用由插件判断（它才知道自家引擎支持哪些 MIME）
    if !matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "flac" | "ogg") {
        return Err("样本仅支持 mp3 / wav / m4a / flac / ogg 格式".into());
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
    // 文件一落地就告知插件：复制成功而后续设置写入失败时，磁盘上已经有样本，
    // 让 env 与文件保持同一时序，插件不会出现「有样本文件却读不到路径」
    announce_clone_sample(&dest); // dest 是上面复制出来的样本绝对路径

    // 连续写两个键，第二次 update 返回的即最终状态
    crate::storage::settings::update_setting(config_dir, "clone_voice_path", serde_json::json!(rel_path))
        .map_err(|e| format!("{e}"))?;
    let new_settings = crate::storage::settings::update_setting(
        config_dir,
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
    let config_dir = &state.config_dir;

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

    crate::storage::settings::update_setting(config_dir, "clone_voice_path", serde_json::json!(""))
        .map_err(|e| format!("{e}"))?;
    let new_settings = crate::storage::settings::update_setting(
        config_dir,
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

    clear_clone_sample();
    notify_changed(&app, EVENT_SETTINGS_CHANGED);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn 样本env设置与移除() {
        // 设计 §六 的「导入后 env 存在、删除后 env 移除」：helper 是纯 env 副作用，
        // 直接给它的契约定测试（命令本身需 AppHandle/State，不适合单测）。
        // 全仓只有本测试读写 VA_CLONE_SAMPLE，并行跑不会互相干扰。
        crate::commands::clone_voice::announce_clone_sample(std::path::Path::new("C:/va-test/clone.wav"));
        assert_eq!(std::env::var("VA_CLONE_SAMPLE").unwrap(), "C:/va-test/clone.wav");
        crate::commands::clone_voice::clear_clone_sample();
        assert!(std::env::var("VA_CLONE_SAMPLE").is_err(), "删除样本后必须撤销告知");
    }
}