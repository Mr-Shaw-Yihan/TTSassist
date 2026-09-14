// 音频路径解析命令：把相对 audio 路径转成绝对路径，供前端 convertFileSrc 用。
//
// 本命令是前端播放链的**唯一咽喉**（getAudioUrl / getAudioAbsPath / revealAudio /
// playToMic 四条链都经它）：效果器开启时在这里按需产出 .fx 缓存路径，
// 主窗 / 快速输入 / 字幕 / 悬浮球四窗口 + 虚拟麦克风 + 遥控全覆盖（设计 §三）。

use tauri::{AppHandle, Manager, State};
use crate::commands::AppState;
use crate::fx::pipeline;
use crate::plugins::PluginManager;

/// 接收 messages/favorites 中存的相对音频路径（如 "audio/m_xxx.wav"），
/// 拼上 data_dir 返回绝对路径；效果开启时返回效果缓存文件的绝对路径。
///
/// `apply_fx`：缺省 true（正常播放）；`revealAudio`（"打开位置"）显式传 false——
/// 资源管理器必须落在原文件而不是 .fx 缓存。
#[tauri::command]
pub async fn resolve_audio_url(
    rel_path: String,
    apply_fx: Option<bool>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let data_dir = state.data_dir.clone();
    let original_abs = data_dir.join(&rel_path);
    if apply_fx == Some(false) {
        return Ok(original_abs.to_string_lossy().into_owned());
    }

    let (preset, params) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        (s.fx_preset.clone(), s.fx_params.clone())
    };
    if preset.is_empty() || preset == pipeline::FX_PRESET_OFF {
        return Ok(original_abs.to_string_lossy().into_owned());
    }
    // 只把需要的插件 Arc 拿出来跨线程（PluginManager 本体不能 move 进 blocking）
    let plugin = preset
        .split_once(':')
        .map(|(pid, _)| pid)
        .and_then(|pid| app.state::<PluginManager>().get_fx(pid));

    // 解码+FFI 是阻塞活 → blocking 线程（管线内部自带 10s 超时，fail-open 直通）
    let resolved = tauri::async_runtime::spawn_blocking(move || {
        pipeline::resolve_with_plugin(&data_dir, &rel_path, &preset, plugin, &params)
    })
    .await
    .map_err(|e| format!("效果处理任务失败: {e}"))?;

    Ok(resolved.to_string_lossy().into_owned())
}
