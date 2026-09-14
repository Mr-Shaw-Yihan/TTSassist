// 语音效果器：合成语音播放前的预设效果处理（audio_effect 插件类型）。
//
// 架构（doc/语音效果器插件设计.md v2 §三）：原文件永不修改，播放侧按需处理并缓存——
// 效果关闭/非法预设直通原文件；开启时查缓存 audio/.fx/<stem>.<指纹>.wav，
// 未命中则 rodio 解码 f32 → 插件 va_fx_process（10s 超时）→ hound 编码 16-bit WAV 落盘。

pub mod codec;
pub mod pipeline;

use std::path::{Path, PathBuf};

/// 虚拟麦克风播放点共用解析（tts.rs / hotkey.rs / bridge.rs 三处 mic.play 之前调用）：
/// 读设置 fx_preset/fx_params → 取已加载效果器插件 → 管线解析。
/// 同步阻塞（内部自带 10s 超时与 fail-open，永不失败永不 panic）；
/// async 上下文请包 spawn_blocking 调用（见 commands/tts.rs）。
pub fn resolve_for_mic(app: &tauri::AppHandle, data_dir: &Path, rel_path: &str) -> PathBuf {
    use tauri::Manager;

    let fallback = || data_dir.join(rel_path);
    let Some(state) = app.try_state::<crate::commands::AppState>() else {
        return fallback();
    };
    let (preset, params) = match state.settings.read() {
        Ok(s) => (s.fx_preset.clone(), s.fx_params.clone()),
        Err(_) => return fallback(),
    };
    if preset.is_empty() || preset == pipeline::FX_PRESET_OFF {
        return fallback();
    }
    let plugin = preset
        .split_once(':')
        .map(|(pid, _)| pid)
        .and_then(|pid| app.try_state::<crate::plugins::PluginManager>().and_then(|pm| pm.get_fx(pid)));
    pipeline::resolve_with_plugin(data_dir, rel_path, &preset, plugin, &params)
}
