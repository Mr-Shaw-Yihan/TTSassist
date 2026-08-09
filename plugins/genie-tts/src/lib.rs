// Genie TTS 本地推理插件（GPT-SoVITS ONNX 引擎，Sidecar 架构）：
//
//   VoiceAssist 宿主
//     └─ 本 dll（薄壳：环境引导 + 进程管理 + HTTP 客户端）
//          └─ genie_server.py（内嵌 Python 子进程，FastAPI）
//               └─ genie_tts（ONNX 推理，纯 CPU）
//
// 环境安装（setup.rs）：宿主可在用户选择引擎/音色时主动触发 run_setup
// （带进度回调），状态经 setup_status 查询；合成入口保留环境（Python/
// GenieData）的静默兜底，但【音色】不再静默下载——未安装音色直接报错
// 引导用户去音色管理安装（阶段 21 设计 21.2.4）。
//
// 音色管理（voicemgmt.rs）：安装/卸载/预加载/导入自定义音色包，
// 经 va_tts_plugin_voices! 导出四个可选符号。
//
// 合成链路（全部幂等）：
//   1. bootstrap：内嵌 Python + pip + jieba_fast(内嵌 wheel) + genie-tts
//   2. server：拉起/探活 genie_server.py 子进程
//   3. ensure_resources：GenieData（首次约 400MB，走 HF 镜像）
//   4. load_character：音色包加载（已装音色只加载权重）
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
mod voicemgmt;
mod voices;

plugin_api::va_tts_plugin! {
    id: "genie-tts",
    name: "Genie TTS（本地·离线）",
    version: "0.1.0",
    audio_format: "wav",
    voices: voices::list_voices,   // 动态音色表（磁盘扫描，见 voices.rs）
    synthesize: synthesize,
}

plugin_api::va_tts_plugin_setup! {
    status: setup::setup_status,   // 磁盘探测，快
    setup: setup::run_setup,       // 分阶段安装 + 进度回调
}

plugin_api::va_tts_plugin_voices! {
    install: voicemgmt::install_voice,       // 环境未就绪先补环境，进度如实上报
    uninstall: voicemgmt::uninstall_voice,   // 释放内存 + 删音色包
    preload: voicemgmt::preload_voice,       // 已装音色加载权重，不触发下载
    import: voicemgmt::import_voice_pack,    // 校验布局后复制进 characters/
}

/// 合成入口：音色安装检查 → 环境补齐 → 合成。
/// 错误一律返回中文消息（会直接展示给用户）。
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("文本不能为空".into());
    }

    // 音色未安装 → 报错引导，不静默下载（阶段 21：下载必须经用户确认 + 进度可见）。
    // 引擎环境（Python/GenieData）的静默兜底保留，见下方 ensure_all。
    let voice_id = voice.unwrap_or(voices::DEFAULT_VOICE);
    if !voices::installed_pack_ids().iter().any(|id| id == voice_id) {
        return Err(format!(
            "音色「{}」尚未下载，请到 设置 → 音色管理 中安装后再试",
            voices::display_label(voice_id)
        ));
    }

    let ctx = paths::Ctx::get()?;
    // 引导与合成全程串行：防止并发请求同时跑首次下载
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    // 环境补齐（与主动安装共用同一链路，无进度回调）
    let opts = setup::SetupOptions {
        voice: Some(voice_id.to_string()),
    };
    let port = setup::ensure_all(ctx, &opts, None)?;

    // 合成
    client::tts(port, voice_id, text)
}
