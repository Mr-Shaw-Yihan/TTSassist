// 音频监听字幕引擎（插件 A：系统音频 → VAD → ASR → 浮窗字幕）。
//
// 与「语音输入」（插件 B，麦克风按住说话）共用 ASR 插件层，但音频来源不同：
// 这里捕获的是**系统播放输出**（游戏/队友语音），经 VAD 切句后送 ASR 转写，
// 结果通过 `asr:subtitle` 事件推给字幕浮窗。
//
// 子模块：
// - vad：能量语音活动检测 + 句子切割（纯 Rust，可单测）
// - capture：WASAPI loopback 采集（系统混音 → 16k 单声道 f32）
// - processes：枚举正在发声的进程（AudioSessionManager2）
// - session：编排 采集 → VAD → WAV → ASR 插件 → emit

// 能量 VAD（纯 Rust，跨平台）
pub mod vad;

// capture / processes / session 依赖 windows crate（仅 Windows 编译）。
#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod processes;
#[cfg(windows)]
pub mod session;

use serde::{Deserialize, Serialize};

/// 事件名：后端把一句转写结果推给前端字幕浮窗。
pub const ASR_SUBTITLE_EVENT: &str = "asr:subtitle";

/// 目标采样率/位深/声道（ASR 插件约定：16kHz / 16bit / mono WAV）
pub const TARGET_SAMPLE_RATE: u32 = 16000;

/// 一个正在发声的音频会话对应的进程（下拉列表条目）
#[derive(Debug, Clone, Serialize)]
pub struct AudioProcess {
    pub pid: u32,
    /// 进程 exe 文件名，如 "FortniteClient-Win64-Shipping.exe"
    pub name: String,
    /// 友好显示名（窗口标题 / 会话显示名，尽力获取；取不到回退 name）
    pub display_name: String,
    /// 当前是否有音频活动（未静音且音量非零）
    pub is_active: bool,
}

/// 一条字幕（推给浮窗 + 管理页回看）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleLine {
    pub text: String,
    /// Unix 毫秒时间戳
    pub ts: u64,
}

/// 一次监听会话的记录（落盘 / 回看 / 导出用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleSession {
    pub started_ts: u64,
    pub ended_ts: u64,
    /// 监听目标进程名（可为空 = 系统混音整体）
    pub process: String,
    pub lines: Vec<SubtitleLine>,
}

/// 会话配置（start_audio_listener 时快照，避免运行中读锁）
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub plugin_id: String,
    /// 语言码；"auto" 或空 → 传 None 让插件自动检测
    pub language: String,
    pub vad: vad::VadConfig,
}

impl SessionConfig {
    /// 归一化语言：auto/空 → None
    pub fn asr_language(&self) -> Option<&str> {
        match self.language.as_str() {
            "" | "auto" | "Auto" | "自动检测" => None,
            other => Some(other),
        }
    }
}

/// COM 初始化守卫（仅 Windows）。在采集线程 / 进程枚举命令线程各自 new 一个，
/// 线程退出时自动 CoUninitialize。
///
/// 若该线程已被别的模式初始化（RPC_E_CHANGED_MODE），视为不拥有 COM，
/// drop 时不调 CoUninitialize，避免把别人的初始化计数打乱。
#[cfg(windows)]
pub(crate) struct ComGuard {
    owned: bool,
}

#[cfg(windows)]
impl ComGuard {
    pub fn new_mta() -> Self {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        // S_OK(0)/S_FALSE(1) = 本线程成功初始化（首次/已同模式重复）
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let ok = hr.is_ok() || hr.0 == 1; // hr.0 == S_FALSE(1) 也算拥有（需配平）
        Self { owned: ok }
    }
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}
