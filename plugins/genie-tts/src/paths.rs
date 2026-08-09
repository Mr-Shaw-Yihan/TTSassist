// 数据目录与配置定位。
//
// 宿主加载插件时设置环境变量 VA_PLUGIN_DATA_DIR_GENIE_TTS（指向
// <插件目录>/data），本模块首次使用时读取并缓存。全部运行期产物
// （Python 运行时 / Genie 资源 / 音色包 / 服务端脚本）都放在该目录下。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 插件 id（manifest 一致）
pub const PLUGIN_ID: &str = "genie-tts";

/// 宿主注入的数据目录环境变量名（规则见宿主 loader.rs：大写 + 连字符转下划线）
const DATA_DIR_ENV: &str = "VA_PLUGIN_DATA_DIR_GENIE_TTS";

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

    /// 音色包根目录（characters/<音色id>/）
    pub fn characters_dir(&self) -> PathBuf {
        self.data_dir.join("characters")
    }

    /// 服务端脚本（运行期由 dll 内嵌源码写出）
    pub fn server_script(&self) -> PathBuf {
        self.data_dir.join("genie_server.py")
    }

    /// 服务端日志
    pub fn server_log(&self) -> PathBuf {
        self.data_dir.join("genie-server.log")
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

// ── genie-config.json（用户可编辑的网络配置）──────────────

/// 插件配置（数据目录下的 genie-config.json，缺失时写出默认值）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenieConfig {
    /// HuggingFace 端点。中国大陆访问 huggingface.co 基本不可达，默认 hf-mirror.com
    #[serde(default = "default_hf_endpoint")]
    pub hf_endpoint: String,
    /// pip 安装源（空 = 官方 PyPI）。默认清华源，国内下载快
    #[serde(default = "default_pip_index")]
    pub pip_index_url: String,
    /// 是否下载中文 RoBERTa 韵律增强资源（fp32 约 1.3GB）。默认关，按需开启
    #[serde(default)]
    pub download_roberta: bool,
}

fn default_hf_endpoint() -> String {
    "https://hf-mirror.com".to_string()
}

fn default_pip_index() -> String {
    "https://pypi.tuna.tsinghua.edu.cn/simple".to_string()
}

impl GenieConfig {
    /// 读取配置；文件不存在则写一份默认配置
    pub fn load_or_init(data_dir: &Path) -> Self {
        let path = data_dir.join("genie-config.json");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<GenieConfig>(raw.trim_start_matches('\u{FEFF}')) {
                return cfg;
            }
            eprintln!("genie-config.json 解析失败，使用默认配置");
        }
        let cfg = GenieConfig {
            hf_endpoint: default_hf_endpoint(),
            pip_index_url: default_pip_index(),
            download_roberta: false,
        };
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&path, json);
        }
        cfg
    }
}
