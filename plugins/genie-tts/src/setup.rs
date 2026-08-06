// 环境安装（setup）能力：状态探测 + 安装编排。
//
// 状态探测（setup_status）纯磁盘检查，快、不触网，宿主列表页会频繁查询。
// 安装编排（run_setup）分阶段推进并经进度回调上报；合成入口（lib.rs）
// 复用同一条 ensure 链路作为兜底（用户跳过主动下载时首次合成仍会自动补齐）。

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
    /// 全就绪：环境 + 资源 + 至少一个音色，可离线合成
    ready: bool,
    /// Python 运行时 + 依赖库已装
    env_ready: bool,
    /// GenieData 语音资源已下载
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

/// 磁盘探测各组件就绪情况
fn probe(ctx: &Ctx) -> SetupStatus {
    // Python 运行时 + 依赖：以标记文件 + site-packages 目录判断（避免起进程）
    let marker_ok = std::fs::read_to_string(ctx.python_dir().join("EMBED_VERSION"))
        .map(|s| s.trim() == bootstrap::PY_VERSION)
        .unwrap_or(false);
    let site = ctx.python_dir().join("Lib").join("site-packages");
    let env_ready = ctx.python_exe().exists()
        && marker_ok
        && site.join("pip").exists()
        && site.join("jieba_fast").exists()
        && site.join("genie_tts").exists();

    // GenieData：hubert 目录 + speaker_encoder.onnx 是最小可运行标志
    let genie_data = ctx.data_dir.join("GenieData");
    let resources_ready = genie_data.join("chinese-hubert-base").is_dir()
        && genie_data.join("speaker_encoder.onnx").is_file();

    // 已安装音色包
    let voices = crate::voices::installed_pack_ids();

    let ready = env_ready && resources_ready && !voices.is_empty();
    let summary = if ready {
        "环境就绪，可离线使用".to_string()
    } else if !env_ready {
        "运行环境未安装（约 250MB）".to_string()
    } else if !resources_ready {
        "语音资源未下载（约 400MB）".to_string()
    } else {
        "尚未安装音色（约 200MB/个）".to_string()
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
/// 进度分段：0-40 运行环境 → 40-70 语音资源 → 70-100 音色。
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
        "环境安装完成，音色「{voice}」已就绪，现在可以离线使用了"
    ))
}

/// 完整环境补齐链路：运行环境 → 服务 → GenieData → 音色。
/// 合成入口（lib.rs）也调它作为自动引导兜底（cb 传 None）。
pub fn ensure_all(ctx: &Ctx, opts: &SetupOptions, cb: ProgressCb) -> Result<u16, String> {
    let report = |pct: f32, msg: &str| {
        if let Some(f) = cb {
            f(pct, msg);
        }
    };

    let cfg = crate::paths::GenieConfig::load_or_init(&ctx.data_dir);

    // 1. Python 运行时 + 依赖（进度 0~40）
    bootstrap::ensure_python_runtime(ctx, &cfg, cb)?;

    // 2. 服务子进程
    report(42.0, "正在启动语音服务…");
    let port = crate::server::ensure_server(ctx)?;

    // 3. GenieData 语音资源（进度 40~70，服务端幂等，已就绪时秒回）
    report(-1.0, "正在检查语音资源（首次约 400MB）…");
    crate::client::ensure_resources(port)?;
    report(70.0, "语音资源就绪");

    // 4. 音色（预置角色缺失时服务端自动下载，进度 70~100）
    let voice = opts.voice.as_deref().unwrap_or(voices::DEFAULT_VOICE);
    report(-1.0, &format!("正在准备音色「{voice}」（首次约 200MB）…"));
    crate::client::load_character(port, voice)?;
    report(98.0, "音色就绪");

    Ok(port)
}
