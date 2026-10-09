// generate_tts 命令：前端输入文本 → TTS 生成 → 写 messages.json → 广播。
// 支持多引擎分发：settings.tts_engine = 内置引擎（moss）| 插件 id

use tauri::{AppHandle, Manager};
use crate::commands::AppState;
use crate::plugins::PluginManager;
use crate::storage::types::{Message, gen_id, now_iso};
use crate::tts::moss::MossEngine;
use crate::tts::traits::{TTSEngine, TTSParams};
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};

#[tauri::command]
pub async fn generate_tts(text: String, app: AppHandle) -> Result<Message, String> {
    generate_tts_impl(&app, &text).await
}

/// 合成中的标志守卫：进入合成置位，任何路径退出（含错误）自动复位。
/// 状态供宿主能力桥 get_state 查询（手机遥控等订阅方据此显示「合成中…」）。
/// 流式管线（tts_stream.rs）复用同一标志。
pub(crate) struct SynthesizingFlag(pub(crate) AppHandle);
impl Drop for SynthesizingFlag {
    fn drop(&mut self) {
        crate::plugins::bridge::set_synthesizing(&self.0, false);
    }
}

/// generate_tts 核心实现（Tauri 命令与宿主能力桥共用）：
/// 读设置 → 引擎分发合成 → 写消息记录 → 广播 → 开关开启时发虚拟麦克风。
/// 扬声器播放由调用方负责（前端命令路径自行播放；桥路径经 playback:play 事件触发）。
pub async fn generate_tts_impl(app: &AppHandle, text: &str) -> Result<Message, String> {
    // [perf] T2 埋点：tts_first_audio 起点（命令进入）
    let t0 = std::time::Instant::now();
    let state = app.state::<AppState>();
    let mic = app.state::<crate::commands::mic::MicPlayback>();
    let plugins = app.state::<PluginManager>();

    let _synth_flag = SynthesizingFlag(app.clone());
    crate::plugins::bridge::set_synthesizing(app, true);

    let data_dir = state.data_dir.clone();

    // 1. 读设置
    let (tts_engine, moss_key, moss_voice, plugin_voices, mic_send_enabled, mic_device, mic_volume) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        (
            s.tts_engine.clone(),
            s.moss_api_key.clone(),
            s.moss_voice_id.clone(),
            s.plugin_voices.clone(),
            s.mic_send_enabled,
            s.mic_output_device.clone(),
            s.mic_playback_volume,
        )
    };

    // 2. 据引擎分发：内置只剩 moss，其余引擎名一律按插件 id 查表
    let audio_path = match tts_engine.as_str() {
        "moss" => {
            if moss_key.is_empty() {
                return Err("请在设置中填写 Moss-TTS API Key".into());
            }
            if moss_voice.is_empty() {
                return Err("请在设置中填写 Moss-TTS voice_id".into());
            }
            let engine = MossEngine::new(moss_key, moss_voice, data_dir.clone());
            engine.generate(&text).await.map_err(|e| format!("{e}"))?
        }
        plugin_id => {
            // 其余引擎名按插件 id 查找
            let engine = plugins
                .build_engine(plugin_id, &data_dir)
                .ok_or_else(|| {
                    format!("引擎「{plugin_id}」不可用：插件未安装或加载失败，请在插件管理中检查")
                })?;
            let voice = plugin_voices
                .get(plugin_id)
                .filter(|v| !v.is_empty())
                .map(|s| s.as_str());
            let params = TTSParams {
                text: &text,
                voice,
                instruction: None,
            };
            engine.generate(params).await.map_err(|e| format!("{e}"))?
        }
    };

    // [perf] T2 埋点：引擎返回 = 音频文件已写完可播放（不含播放设备缓冲延迟）。
    // cache 恒为 miss：当前 generate_tts 无缓存/收藏复用路径（收藏播放由前端直播已有
    // 音频文件，不进本命令）；保留该字段，防止将来引入缓存后两条路径的数字被混读。
    // 只记长度不记内容（隐私规则 §3.4）
    log_info!(
        // allowlog: 参数只取 text.chars().count() 的字符数（len 字段），文本内容不进日志
        "{}",
        crate::perf::perf_line(
            "tts_first_audio",
            t0.elapsed().as_millis() as u64,
            &[
                ("engine", tts_engine.as_str()),
                ("cache", "miss"),
                ("len", &text.chars().count().to_string()),
            ],
        )
    );

    // 3. 保存消息记录
    let message = Message {
        id: gen_id("m"),
        content: text.to_string(),
        audio_path,
        created_at: now_iso(),
    };
    let result = message.clone();
    crate::storage::messages::add_message(&data_dir, message)
        .map_err(|e| format!("保存消息失败: {e}"))?;

    // 4. 广播事件
    notify_changed(&app, EVENT_MESSAGE_CHANGED);

    // 5. 全局开关开启且配置了麦克风设备 → 同时发到虚拟麦克风（扬声器播放由前端负责）
    if mic_send_enabled && !mic_device.is_empty() {
        let abs = data_dir.join(&result.audio_path);
        mic.play(abs, mic_device, mic_volume);
    }

    Ok(result)
}