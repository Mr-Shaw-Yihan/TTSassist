// 环境安装（setup）能力：状态探测 + 安装编排。
//
// 状态探测（setup_status）纯磁盘检查，快、不触网，宿主列表页会频繁查询。
// 安装编排（run_setup）分阶段推进并经进度回调上报；合成入口（lib.rs）
// 复用同一条 ensure 链路作为兜底（用户跳过主动下载时首次合成仍会自动补齐
// 环境与模型；音色不静默下载，见 lib.rs）。

use crate::bootstrap::{self, ProgressCb};
use crate::paths::Ctx;
use crate::voices;

/// setup options（run_setup 的 JSON 入参；合成入口也直接构造它）
#[derive(Debug, Default, serde::Deserialize)]
pub struct SetupOptions {
    /// 要确保安装的音色 id（缺省 = 默认音色）
    pub voice: Option<String>,
}

/// 环境状态（序列化给宿主，字段名与前端约定一致）
#[derive(Debug, serde::Serialize)]
struct SetupStatus {
    /// 全就绪：环境 + 模型 + 至少一个音色，可离线合成
    ready: bool,
    /// Python 运行时 + 依赖库已装
    env_ready: bool,
    /// ONNX 模型已下载
    resources_ready: bool,
    /// 已安装的音色包 id
    voices: Vec<String>,
    /// 人类可读摘要
    summary: String,
}

/// 探测环境状态（纯磁盘检查）
pub fn setup_status() -> String {
    let status = match Ctx::get() {
        Ok(ctx) => probe(ctx),
        Err(e) => SetupStatus {
            ready: false,
            env_ready: false,
            resources_ready: false,
            voices: Vec::new(),
            summary: e,
        },
    };
    serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
}

/// ONNX 模型目录必备文件（与服务端 hojo_server.py 的 MODEL_FILES 一致）
const MODEL_FILES: &[&str] = &[
    "Hojo-TTS-Light-llm.onnx",
    "Hojo-TTS-Light-encoder.onnx",
    "Hojo-TTS-Light-decoder.onnx",
    "Hojo-TTS-Light-speaker.onnx",
    "Hojo-TTS-Light-voice.npz",
    "config.json",
    "tokenizer.json",
    "tokenizer_config.json",
];

/// 磁盘探测各组件就绪情况
fn probe(ctx: &Ctx) -> SetupStatus {
    // Python 运行时 + 依赖：以标记文件 + 关键包目录判断（避免起进程）
    let marker_ok = std::fs::read_to_string(ctx.python_dir().join("EMBED_VERSION"))
        .map(|s| s.trim() == bootstrap::PY_VERSION)
        .unwrap_or(false);
    let deps_ok = std::fs::read_to_string(ctx.python_dir().join("DEPS_VERSION"))
        .map(|s| s.trim() == bootstrap::DEPS_VERSION)
        .unwrap_or(false);
    let site = ctx.python_dir().join("Lib").join("site-packages");
    let env_ready = ctx.python_exe().exists()
        && marker_ok
        && deps_ok
        && site.join("pip").exists()
        && site.join("fastapi").exists()
        && site.join("onnxruntime").exists()
        && site.join("huggingface_hub").is_dir();

    // 模型：8 个必备文件齐全
    let resources_ready = MODEL_FILES
        .iter()
        .all(|name| ctx.models_dir().join(name).is_file());

    // 已安装音色包
    let voices = crate::voices::installed_pack_ids();

    let ready = env_ready && resources_ready && !voices.is_empty();
    let summary = if ready {
        "环境就绪，可离线使用".to_string()
    } else if !env_ready {
        "运行环境未安装（约 1.2GB）".to_string()
    } else if !resources_ready {
        "语音模型未下载（约 460MB）".to_string()
    } else {
        "尚未安装音色（预置音色几百 KB/个）".to_string()
    };

    SetupStatus {
        ready,
        env_ready,
        resources_ready,
        voices,
        summary,
    }
}

/// 执行环境安装/补齐（幂等）。
/// options：JSON（{"voice":"..."} 指定要确保的音色）；cb：进度回调。
/// 进度分段：0-40 运行环境 → 40-45 服务 → 45-85 模型 → 85-100 音色。
pub fn run_setup(options: Option<&str>, cb: &dyn Fn(f32, &str)) -> Result<String, String> {
    let ctx = Ctx::get()?;
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    let opts: SetupOptions = options
        .map(|s| serde_json::from_str(s).unwrap_or_default())
        .unwrap_or_default();

    ensure_all(ctx, &opts, Some(cb))?;

    cb(100.0, "安装完成");
    let voice = opts.voice.as_deref().unwrap_or(voices::DEFAULT_VOICE);
    Ok(format!(
        "环境安装完成，音色「{}」已就绪，现在可以离线使用了",
        voices::display_label(voice)
    ))
}

/// 完整环境补齐链路：运行环境 → 服务 → 模型 → 音色。
/// 合成入口（lib.rs）也调它作为自动引导兜底（cb 传 None）。
/// 注意：音色环节只对【已安装】音色做加载；预置音色缺失时仅 run_setup /
/// install_voice 这类显式安装流程会下载（调用方负责先落盘再进来）。
pub fn ensure_all(ctx: &Ctx, opts: &SetupOptions, cb: ProgressCb) -> Result<u16, String> {
    let report = |pct: f32, msg: &str| {
        if let Some(f) = cb {
            f(pct, msg);
        }
    };

    // 1. Python 运行时 + 依赖（进度 0~40）
    bootstrap::ensure_python_runtime(ctx, cb)?;

    // 2. 服务子进程
    report(42.0, "正在启动语音服务…");
    let port = crate::server::ensure_server(ctx)?;

    // 3. ONNX 模型（进度 45~85，服务端幂等 + 多端点回退，已就绪时秒回）
    report(-1.0, "正在检查语音模型（首次约 460MB）…");
    crate::client::ensure_models(port)?;
    report(85.0, "语音模型就绪");

    // 4. 音色（预置角色缺失时先从上游下载参考音频，几百 KB；
    //    用户自备音色缺失则报错引导导入）。然后预加载推理引擎（首次含模型加载）
    let voice = opts.voice.as_deref().unwrap_or(voices::DEFAULT_VOICE);
    report(
        -1.0,
        &format!("正在准备音色「{}」…", voices::display_label(voice)),
    );
    crate::voicemgmt::ensure_voice_pack(ctx, voice)?;
    crate::client::load_voice(port, voice)?;
    report(98.0, "音色就绪");

    Ok(port)
}
