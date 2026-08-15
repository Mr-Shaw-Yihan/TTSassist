// MiniMax TTS 插件（国内版）— 调用 api.minimaxi.com 云端语音合成。
//
// 环境变量：MINIMAX_API_KEY（必填，从 https://platform.minimaxi.com 获取）
// 模型：speech-2.8-hd（默认），支持 speech-2.8-turbo / speech-02-hd 等
// 输出：MP3 32kHz 128kbps 单声道

plugin_api::va_tts_plugin! {
    id: "minimax-tts",
    name: "MiniMax TTS（国内版）",
    version: "0.2.0",
    audio_format: "mp3",
    voices: minimax_tts_core::voices_list,
    synthesize: synthesize,
}

/// 国内版 API 端点
const BASE_URL: &str = "https://api.minimaxi.com";

/// 文本 → MP3 字节（voice 为 None 用默认甜美女性）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    minimax_tts_core::synthesize(BASE_URL, "MINIMAX_API_KEY", text, voice)
}
