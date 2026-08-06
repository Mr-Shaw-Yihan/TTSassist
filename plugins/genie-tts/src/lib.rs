// Genie TTS 本地推理插件（GPT-SoVITS ONNX 引擎，Sidecar 架构）：
//
//   VoiceAssist 宿主
//     └─ 本 dll（薄壳：环境引导 + 进程管理 + HTTP 客户端）
//          └─ genie_server.py（内嵌 Python 子进程，FastAPI）
//               └─ genie_tts（ONNX 推理，纯 CPU）
//
// 环境安装（setup.rs）：宿主可在用户选择引擎/音色时主动触发 run_setup
// （带进度回调），状态经 setup_status 查询；合成入口保留同一条 ensure
// 链路作为兜底（用户跳过主动下载时，首次合成仍会自动补齐）。
//
// 合成链路（全部幂等）：
//   1. bootstrap：内嵌 Python + pip + jieba_fast(内嵌 wheel) + genie-tts
//   2. server：拉起/探活 genie_server.py 子进程
//   3. ensure_resources：GenieData（首次约 400MB，走 HF 镜像）
//   4. load_character：音色包加载（预置角色首次自动下载约 200MB）
//   5. tts：文本 → WAV（32kHz 单声道 16bit）
//
// 音色扩展：把 GPT-SoVITS ONNX 音色包放进数据目录 characters/<名字>/ 即可，
// 音色表动态扫描（voices 宏分支），无需重启应用。

mod bootstrap;
mod client;
mod paths;
mod server;
mod setup;
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

plugin_api::va_tts_plugin_setup! {
    status: setup::setup_status,   // 磁盘探测，快
    setup: setup::run_setup,       // 分阶段安装 + 进度回调
}

/// 合成入口：环境补齐 → 合成。
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

    // 环境补齐（与主动安装共用同一链路，无进度回调）
    let opts = setup::SetupOptions {
        voice: voice.map(str::to_string),
    };
    let port = setup::ensure_all(ctx, &opts, None)?;

    // 合成
    let voice_id = voice.unwrap_or(voices::DEFAULT_VOICE);
    client::tts(port, voice_id, text)
}
