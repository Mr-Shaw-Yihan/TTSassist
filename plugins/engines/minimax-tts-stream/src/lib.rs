// MiniMax 流式 TTS 插件 — WebSocket T2A v2 边合成边出块（国内版）。
//
// 与 minimax-tts（非流式）并存，供用户对比首响延迟与播放流畅度：
// - 流式（本插件主路径）：WS task_start → task_continue → 逐块 PCM（32kHz/16-bit/单声道），
//   宿主收到一块播一块，首响为服务端首个音频块到达时间；
// - 阻塞（老宿主兜底）：HTTP t2a_v2 一次成段 MP3（与 minimax-tts 同路径）。
//
// 音色：系统音色 + 账号克隆音色均可（voice_id 原样透传进 task_start，
//   国内端已确认提供 voice_clone/get_voice API，克隆功能由宿主「音色管理」面板接入）。
//
// 配置（宿主设置面板按 manifest.config 注入环境变量，改完下次合成即生效）：
//   MINIMAX_STREAM_API_KEY  API Key（必填，从 platform.minimaxi.com 获取）
//   MINIMAX_STREAM_MODEL    模型（默认 speech-2.8-hd）

plugin_api::va_tts_plugin! {
    id: "minimax-tts-stream",
    name: "MiniMax TTS（国内版）",
    version: "0.3.1",
    audio_format: "mp3",
    voices: minimax_tts_core::voices_list,
    synthesize: synthesize,
}

plugin_api::va_tts_stream_plugin! {
    start: stream_start,
    next: stream_next,
    stop: stream_stop,
}

/// 国内版 HTTP / WS 端点前缀（本插件只走国内版：国际版与国内版是两个账号，
/// 单处填 Key 无法兼顾，且当前无国际 Key 可测，故剔除国际分支）。
const DOMESTIC_HTTP: &str = "https://api.minimaxi.com";
const DOMESTIC_WS: &str = "wss://api.minimaxi.com";

/// 默认模型（与 minimax-tts-core::DEFAULT_MODEL 一致；显式写出便于对照配置项）
const DEFAULT_MODEL: &str = "speech-2.8-hd";

/// 读模型配置（空值回退默认）
fn model() -> String {
    let raw = std::env::var("MINIMAX_STREAM_MODEL").unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 读 API Key（宿主注入环境变量；流式插件用独立 env 名，避免与其他
/// MiniMax 插件的必填配置触发宿主 env 冲突检测）
fn read_api_key() -> Result<String, String> {
    std::env::var("MINIMAX_STREAM_API_KEY")
        .map(|k| k.trim().to_string())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            "未配置 MiniMax API Key：请在插件设置中填写（platform.minimaxi.com 获取）".to_string()
        })
}

// ── 阻塞兜底（老宿主无流式符号时走此路径）─────────────────

/// 文本 → MP3 字节（HTTP t2a_v2；模型口径与流式路径一致）
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    read_api_key()?; // 先行校验，保证两条路径的报错口径一致（Key 实读在 core 内）
    minimax_tts_core::synthesize_with_model(
        DOMESTIC_HTTP,
        "MINIMAX_STREAM_API_KEY",
        &model(),
        text,
        voice,
    )
}

// ── 流式主路径 ──────────────────────────────────────────

/// 打开流式会话：连接 WS、握手，返回 (音频块格式自述 JSON, 会话句柄)
fn stream_start(text: &str, voice: Option<&str>) -> Result<(String, u64), String> {
    let api_key = read_api_key()?;
    let session = minimax_tts_core::stream::StreamSession::open(
        DOMESTIC_WS,
        &api_key,
        &model(),
        voice.unwrap_or(minimax_tts_core::DEFAULT_VOICE_ID),
        text,
    )?;
    let id = minimax_tts_core::stream::global_sessions().insert(session);
    Ok((minimax_tts_core::stream::stream_info_json(), id))
}

/// 拉取下一个 PCM 块（Ok 空 vec = 流正常结束）
fn stream_next(session: u64) -> Result<Vec<u8>, String> {
    minimax_tts_core::stream::global_sessions().next_chunk(session)
}

/// 中止并释放会话（幂等）
fn stream_stop(session: u64) {
    minimax_tts_core::stream::global_sessions().abort(session);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 端点常量为国内版() {
        assert_eq!(DOMESTIC_HTTP, "https://api.minimaxi.com");
        assert_eq!(DOMESTIC_WS, "wss://api.minimaxi.com");
    }

    #[test]
    fn model_空值回退默认() {
        std::env::set_var("MINIMAX_STREAM_MODEL", "  ");
        assert_eq!(model(), DEFAULT_MODEL);
        std::env::set_var("MINIMAX_STREAM_MODEL", "speech-2.8-turbo");
        assert_eq!(model(), "speech-2.8-turbo");
        std::env::remove_var("MINIMAX_STREAM_MODEL");
    }

    // 注：read_api_key / stream_start 依赖环境变量注入，归属宿主集成验证
    // （plugins/scripts/test-minimax-stream.ps1），不在此模拟进程级 env。
}
