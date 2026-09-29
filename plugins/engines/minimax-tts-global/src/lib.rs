// MiniMax TTS 插件（国际版）— 调用 api-uw.minimax.io 云端语音合成。
//
// 环境变量：MINIMAX_GLOBAL_API_KEY（推荐）或 MINIMAX_API_KEY（后备）
//          从 https://www.minimax.io 控制台获取
// 模型：speech-2.8-hd（默认），支持 speech-2.8-turbo / speech-02-hd 等
// 输出：MP3 32kHz 128kbps 单声道
// 注：T2A 走官方低延迟端点 api-uw；克隆/音色管理 API 由宿主走 api.minimax.io

plugin_api::va_tts_plugin! {
    id: "minimax-tts-global",
    name: "MiniMax TTS（国际版）",
    version: "0.2.1",
    audio_format: "mp3",
    voices: minimax_tts_core::voices_list,
    synthesize: synthesize,
}

/// 国际版 T2A 端点（官方低延迟变体，参数与 api.minimax.io 完全一致）
const BASE_URL: &str = "https://api-uw.minimax.io";

/// 文本 → MP3 字节（voice 为 None 用默认甜美女性）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    minimax_tts_core::synthesize(BASE_URL, "MINIMAX_GLOBAL_API_KEY", text, voice)
}
