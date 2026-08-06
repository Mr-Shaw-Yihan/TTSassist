// Genie TTS 本地推理插件（GPT-SoVITS ONNX 引擎，Sidecar 架构）：
//
//   VoiceAssist 宿主
//     └─ 本 dll（薄壳：环境引导 + 进程管理 + HTTP 客户端）
//          └─ genie_server.py（内嵌 Python 子进程，FastAPI）
//               └─ genie_tts（ONNX 推理，纯 CPU）
//
// 合成链路（全部幂等，首次运行自动引导）：
//   1. bootstrap：内嵌 Python + pip + genie-tts（首次约 250MB 下载）
//   2. server：拉起/探活 genie_server.py 子进程
//   3. ensure_resources：GenieData + RoBERTa（首次约 400MB，走 HF 镜像）
//   4. load_character：音色包加载（预置角色首次自动下载约 200MB）
//   5. tts：文本 → WAV（32kHz 单声道 16bit）
//
// 音色扩展：把 GPT-SoVITS ONNX 音色包放进数据目录 characters/<名字>/ 即可，
// 音色表动态扫描（voices 宏分支），无需重启应用。

mod bootstrap;
mod client;
mod paths;
mod server;
mod util;
mod voices;

plugin_api::va_tts_plugin! {
    id: "genie-tts",
    name: "Genie TTS（本地·离线）",
    version: "1.0.0",
    audio_format: "wav",
    voices: voices::list_voices,   // 动态音色表（磁盘扫描，见 voices.rs）
    synthesize: synthesize,
}

/// 合成入口：环境引导 → 服务就绪 → 加载音色 → 合成。
/// 错误一律返回中文消息（会直接展示给用户）。
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("文本不能为空".into());
    }
    let ctx = paths::Ctx::get()?;
    // 引导与合成全程串行：防止并发请求同时跑首次下载
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    let cfg = paths::GenieConfig::load_or_init(&ctx.data_dir);

    // 1. Python 运行时 + genie-tts + 服务端脚本
    bootstrap::ensure_python_runtime(ctx, &cfg)?;

    // 2. 服务子进程
    let port = server::ensure_server(ctx)?;

    // 3. Genie 运行资源（幂等，已就绪时秒回）
    client::ensure_resources(port)?;

    // 4. 音色（预置角色缺失时服务端自动下载）
    let voice_id = voice.unwrap_or(voices::DEFAULT_VOICE);
    client::load_character(port, voice_id)?;

    // 5. 合成
    client::tts(port, voice_id, text)
}
