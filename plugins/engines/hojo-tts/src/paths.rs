// 数据目录与配置定位。
//
// 宿主加载插件时设置环境变量 VA_PLUGIN_DATA_DIR_HOJO_TTS（指向
// <插件目录>/data），本模块首次使用时读取并缓存。全部运行期产物
// （Python 运行时 / ONNX 模型 / 音色包 / 服务端脚本）都放在该目录下。
//
// 面向大众用户，不做任何用户配置界面：网络容错全部内置于插件
// （模型下载多端点回退、pip 安装多源回退）。下方两个环境变量仅供
// 排障/高级用户从外部覆盖（无 UI，不进 manifest config）。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 插件 id（与 manifest 一致）
pub const PLUGIN_ID: &str = "hojo-tts";

/// 宿主注入的数据目录环境变量名（规则见宿主 loader.rs：大写 + 连字符转下划线）
const DATA_DIR_ENV: &str = "VA_PLUGIN_DATA_DIR_HOJO_TTS";

/// 插件全局上下文（数据目录 + 串行锁）
pub struct Ctx {
    /// 数据目录（所有运行时产物的根）
    pub data_dir: PathBuf,
    /// 环境引导/合成的全局串行锁（防止并发合成同时跑引导流程）
    pub ensure_lock: std::sync::Mutex<()>,
}

/// 全局上下文（首次使用时从环境变量解析）
pub static CTX: OnceLock<Ctx> = OnceLock::new();

impl Ctx {
    /// 取全局上下文；宿主未注入数据目录时返回中文错误
    pub fn get() -> Result<&'static Ctx, String> {
        if let Some(ctx) = CTX.get() {
            return Ok(ctx);
        }
        // 首次初始化（并发下重复初始化无副作用，值相同）
        let data_dir = std::env::var(DATA_DIR_ENV)
            .map(PathBuf::from)
            .map_err(|_| {
                format!(
                    "插件「{PLUGIN_ID}」未获得数据目录（环境变量 {DATA_DIR_ENV} 缺失），\
                     请用 VoiceAssist 1.4.0 及以上版本加载本插件"
                )
            })?;
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("创建插件数据目录失败: {e}"))?;
        Ok(CTX.get_or_init(|| Ctx {
            data_dir,
            ensure_lock: std::sync::Mutex::new(()),
        }))
    }

    // ── 常用路径 ──────────────────────────────────────

    /// 内嵌 Python 运行时目录
    pub fn python_dir(&self) -> PathBuf {
        self.data_dir.join("python")
    }

    /// 内嵌 Python 解释器
    pub fn python_exe(&self) -> PathBuf {
        self.python_dir().join("python.exe")
    }

    /// ONNX 模型目录（HF 仓库 HojoAI/Hojo-TTS-Light 快照，约 460MB）
    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    /// 音色包根目录（voices/<音色id>/{ref.wav, voice.json}）
    pub fn voices_dir(&self) -> PathBuf {
        self.data_dir.join("voices")
    }

    /// 服务端脚本（运行期由 dll 内嵌源码写出）
    pub fn server_script(&self) -> PathBuf {
        self.data_dir.join("hojo_server.py")
    }

    /// 上游推理模块（随服务端脚本一并写出，保持上游文件名便于对齐升级）
    pub fn onnx_model_script(&self) -> PathBuf {
        self.data_dir.join("onnx_model.py")
    }

    /// 服务端日志
    pub fn server_log(&self) -> PathBuf {
        self.data_dir.join("hojo-server.log")
    }

    /// 下载缓存目录
    pub fn dl_dir(&self) -> PathBuf {
        self.data_dir.join(".dl")
    }

    /// 端口号文件（记录当前服务端端口，调试用）
    pub fn port_file(&self) -> PathBuf {
        self.data_dir.join("server-port")
    }
}

// ── 环境变量覆盖（排障后门，无 UI；未设置时用内置默认）──────

/// HF 下载端点覆盖（服务端模型下载多端点回退的起点）
pub const ENV_HF_ENDPOINT: &str = "HOJO_TTS_HF_ENDPOINT";
pub const DEFAULT_HF_ENDPOINT: &str = "https://hf-mirror.com";

/// pip 安装源覆盖（设置后 bootstrap 只用它，不再多源回退）
pub const ENV_PIP_INDEX: &str = "HOJO_TTS_PIP_INDEX_URL";

/// HF 端点：环境变量覆盖 > 内置默认（服务端拉起时透传给子进程做下载起点）
pub fn hf_endpoint() -> String {
    match std::env::var(ENV_HF_ENDPOINT) {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => DEFAULT_HF_ENDPOINT.to_string(),
    }
}
