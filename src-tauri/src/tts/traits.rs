// TTS 引擎抽象层：统一接口 + 参数 + 错误类型。
//
// 设计说明（非技术用户可略读）：
// - TTSEngine trait 是所有引擎的"合同"——任何引擎只要实现了 generate() 就能被上层使用
// - TTSParams 把参数包成结构体，以后加字段（语速/风格指令）不改函数签名，不破坏调用方
// - TtsError 枚举分门别类，上层（commands）可据此给用户显示不同中文提示

use thiserror::Error;

// ── 参数 ──────────────────────────────────────────

/// TTS 生成参数。
/// 首版只用 text 和 voice；instruction(风格指令)留作扩展位。
pub struct TTSParams<'a> {
    /// 要合成的文本
    pub text: &'a str,
    /// 音色 id；None 使用引擎默认音色
    pub voice: Option<&'a str>,
    /// 风格指令（首版始终为 None，扩展位）
    pub instruction: Option<&'a str>,
}

impl<'a> TTSParams<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, voice: None, instruction: None }
    }
    pub fn with_voice(self, voice: &'a str) -> Self {
        Self { voice: Some(voice), ..self }
    }
}

// ── 引擎类型 ────────────────────────────────────

pub enum EngineCategory {
    /// 本地引擎（不依赖网络）
    Local,
    /// 远程 API 引擎（需联网）
    Remote,
}

// ── 错误类型 ──────────────────────────────────────

#[derive(Debug, Error)]
pub enum TtsError {
    /// settings 中未配置 API Key
    #[error("未配置 MiMo API Key，请在设置中填写")]
    NoApiKey,

    /// HTTP 网络层错误（连不上、超时等）
    #[error("网络请求失败: {0}")]
    Network(String),

    /// 服务端返回非 2xx（如 key 错 401、欠费、限流等）
    #[error("服务端错误 (HTTP {status}): {body}")]
    Http { status: u16, body: String },

    /// 返回的 base64 音频数据无法解码
    #[error("音频解码失败: {0}")]
    DecodeAudio(String),

    /// 音频文件写入失败
    #[error("音频写入失败: {0}")]
    WriteFile(String),

    /// settings.tts_engine 里配置的引擎名不存在
    #[error("未知的引擎: {0}")]
    UnknownEngine(String),
}

// ── 引擎接口 ────────────────────────────────────

/// 所有 TTS 引擎必须实现此 trait。
///
/// `Send + Sync` 是因为引擎实例会被封装在 `tauri::State` 中跨线程共享。
#[async_trait::async_trait]
pub trait TTSEngine: Send + Sync {
    /// 合成语音。
    ///
    /// 返回音频文件的**相对路径**（相对 app_data_dir，如 "audio/m_xxx.wav"），
    /// 上层可直接用此路径拼接绝对路径播放。
    async fn generate(&self, params: TTSParams<'_>) -> std::result::Result<String, TtsError>;

    /// 引擎名称（如 "mimo"、"local-test"）
    fn name(&self) -> &str;

    /// 引擎类别
    fn category(&self) -> EngineCategory;
}
