// ASR 语音识别模块：负责语音输入的录音、转写、命令注册。
//
// 组成：
// - 本文件：Tauri 命令（前端调用入口）
// - 依赖 plugins 系统的 LoadedAsrPlugin 提供实际转写能力
//
// 设计原则：
// - 宿主负责音频采集（麦克风录音），插件只负责"音频→文本"
// - 录音数据存内存 buffer，不落盘（隐私）
// - 转写为阻塞 FFI 调用，需在 blocking 线程执行

use std::sync::Mutex;
use tauri::State;

use crate::plugins::PluginManager;

/// ASR 录音状态（跨命令共享）
pub struct AsrState {
    /// 录音中的 PCM 数据缓冲（16kHz/16bit/mono）
    pub recording: Mutex<Option<Vec<u8>>>,
    /// 是否正在录音
    pub is_recording: Mutex<bool>,
}

impl AsrState {
    pub fn new() -> Self {
        Self {
            recording: Mutex::new(None),
            is_recording: Mutex::new(false),
        }
    }
}

/// 列出已安装的 ASR 插件
#[tauri::command]
pub fn list_asr_plugins(manager: State<PluginManager>) -> Vec<AsrPluginInfo> {
    let mut result = Vec::new();
    // 遍历 registry 中所有 asr_engine 类型的插件
    let reg = crate::plugins::registry::load_registry(manager.plugins_root());
    for entry in &reg.plugins {
        let dir = manager.plugins_root().join(&entry.id);
        if let Ok(manifest) = crate::plugins::manifest::PluginManifest::load(&dir) {
            if manifest.plugin_type == "asr_engine" {
                let loaded = manager.get_asr(&entry.id).is_some();
                let languages = manager
                    .get_asr(&entry.id)
                    .map(|p| p.languages_json.clone())
                    .unwrap_or_else(|| "[]".to_string());
                result.push(AsrPluginInfo {
                    id: entry.id.clone(),
                    name: manifest.name,
                    version: manifest.version,
                    loaded,
                    languages,
                });
            }
        }
    }
    result
}

/// ASR 插件信息（前端展示用）
#[derive(Debug, Clone, serde::Serialize)]
pub struct AsrPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loaded: bool,
    /// 支持的语言 JSON
    pub languages: String,
}

/// 转写音频（前端录音完成后调用）
/// audio_bytes: WAV 格式的音频数据
/// plugin_id: ASR 插件 id
/// language: 语言代码（如 "zh"）
#[tauri::command]
pub async fn asr_transcribe(
    audio_bytes: Vec<u8>,
    plugin_id: String,
    language: String,
    manager: State<'_, PluginManager>,
) -> Result<String, String> {
    let plugin = manager
        .get_asr(&plugin_id)
        .ok_or_else(|| format!("ASR 插件「{plugin_id}」未加载"))?;

    let lang = if language.is_empty() {
        None
    } else {
        Some(language)
    };

    // FFI 转写是阻塞调用 → 丢到 blocking 线程池
    let result = tauri::async_runtime::spawn_blocking(move || {
        plugin.transcribe(&audio_bytes, lang.as_deref())
    })
    .await
    .map_err(|e| format!("ASR 任务中断: {e}"))?
    .map_err(|e| e.to_string())?;

    Ok(result)
}
