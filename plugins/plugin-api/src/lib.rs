// VoiceAssist 插件接口定义 crate（宿主程序与插件共用）。
//
// 插件 = 一个 .dll（cdylib），通过 C ABI 与主程序通信（跨动态库边界 C ABI 最稳）。
// 本 crate 定义：
// - 导出符号名常量（宿主用 libloading 按名取函数）
// - 各函数的类型别名（宿主侧按此签名解读函数指针）
// - VoiceItem（音色 JSON 条目）
// - va_tts_plugin! 宏（插件侧一键生成全部导出函数，保证签名/内存约定不出错）
//
// 内存约定（重要）：
// - 元信息类函数（id/name/version/audio_format/list_voices）返回插件内【静态】字符串，
//   宿主必须立即拷贝，不得长期持有指针，无需释放。
// - va_tts_synthesize 成功时通过 out_audio/out_len 输出音频（Box<[u8]> 泄漏而来），
//   宿主拷贝后必须调 va_free_bytes(ptr, len) 归还。
// - 失败时错误信息写入 out_err（CString），宿主读取后必须调 va_free_cstr 归还。

use std::ffi::{c_char, CStr, CString};
use std::sync::OnceLock;

/// va_tts_synthesize 返回码：成功
pub const VA_OK: i32 = 0;
/// va_tts_synthesize 返回码：失败（详见 out_err）
pub const VA_ERR: i32 = 1;

// ── 导出符号名（libloading 要求 NUL 结尾字节串）──────────────

pub const SYM_PLUGIN_ID: &[u8] = b"va_plugin_id\0";
pub const SYM_PLUGIN_NAME: &[u8] = b"va_plugin_name\0";
pub const SYM_PLUGIN_VERSION: &[u8] = b"va_plugin_version\0";
pub const SYM_AUDIO_FORMAT: &[u8] = b"va_audio_format\0";
pub const SYM_LIST_VOICES: &[u8] = b"va_list_voices\0";
pub const SYM_TTS_SYNTHESIZE: &[u8] = b"va_tts_synthesize\0";
pub const SYM_FREE_BYTES: &[u8] = b"va_free_bytes\0";
pub const SYM_FREE_CSTR: &[u8] = b"va_free_cstr\0";

// ── ASR 插件导出符号 ──────────────────────────────────────
pub const SYM_ASR_TRANSCRIBE: &[u8] = b"va_asr_transcribe\0";
pub const SYM_ASR_LANGUAGES: &[u8] = b"va_asr_languages\0";

// 可选符号（本地引擎"环境安装"支持；老插件没有，宿主按 Option 处理）
pub const SYM_PLUGIN_SETUP_STATUS: &[u8] = b"va_plugin_setup_status\0";
pub const SYM_PLUGIN_SETUP: &[u8] = b"va_plugin_setup\0";

// 可选符号（本地引擎"音色管理"支持；va_tts_plugin_voices! 宏生成，老插件没有）
pub const SYM_VOICE_INSTALL: &[u8] = b"va_voice_install\0";
pub const SYM_VOICE_UNINSTALL: &[u8] = b"va_voice_uninstall\0";
pub const SYM_VOICE_PRELOAD: &[u8] = b"va_voice_preload\0";
pub const SYM_VOICE_IMPORT: &[u8] = b"va_voice_import\0";

// ── 宿主能力桥（host bridge）─────────────────────────────
//
// 现有符号全部是宿主→插件单向调用；能力桥补上插件→宿主反向调用：
// 宿主加载插件时若发现可选导出符号 va_plugin_attach_host，则传入一张
// 函数指针表（VaHostServices），插件此后通过它调用宿主能力。
// manifest 声明 requires_host_bridge: true 的插件才会被注入（安全白名单），
// 未声明或未导出该符号的插件完全不受影响（老插件无感）。

/// 可选导出符号：宿主能力桥注入点
pub const SYM_PLUGIN_ATTACH_HOST: &[u8] = b"va_plugin_attach_host\0";

/// 能力表版本。宿主按此值构造表；插件发现不认识的主版本应拒绝使用。
pub const VA_HOST_SERVICES_VERSION: u32 = 2;

/// 宿主桥上下文：宿主私有不透明指针，插件调用能力函数时原样传回。
pub type VaHostCtx = *mut std::os::raw::c_void;

/// 事件回调（插件提供给 subscribe_events）：宿主在数据变化时于任意线程调用。
/// event_json 为 NUL 结尾 UTF-8，仅在回调期间有效（插件需立即拷贝）；
/// user_data 为订阅时插件传入的指针。
pub type VaHostEventCallback =
    unsafe extern "C" fn(user_data: *mut std::os::raw::c_void, event_json: *const c_char);

/// 宿主能力表（宿主分配，进程存活期间长期有效；插件不得释放表本身）。
/// 各能力统一约定：成功返回 VA_OK；带 out_json 的写 JSON 字符串（宿主分配），
/// 带出 out_err 的失败时写中文错误消息——两者都用 free_string 归还。
#[repr(C)]
pub struct VaHostServices {
    /// 能力表版本（VA_HOST_SERVICES_VERSION）
    pub version: u32,
    /// 宿主私有上下文，调用任何能力函数时原样传回
    pub ctx: VaHostCtx,
    /// 释放宿主分配的字符串（out_json / out_err）
    pub free_string: unsafe extern "C" fn(ctx: VaHostCtx, ptr: *mut c_char),
    /// 收藏元数据 JSON（不含音频）：
    /// [{"id","note","created_at","hotkey"}]
    pub list_favorites: unsafe extern "C" fn(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32,
    /// 触发收藏播放（与收藏快捷键同逻辑：虚拟麦克风 + 扬声器）
    pub play_favorite:
        unsafe extern "C" fn(ctx: VaHostCtx, id: *const c_char, out_err: *mut *mut c_char) -> i32,
    /// 停止当前播放（扬声器 + 虚拟麦克风）
    pub stop_playback: unsafe extern "C" fn(ctx: VaHostCtx) -> i32,
    /// 文字合成（走宿主现有合成管线：引擎分发 + 消息记录 + 麦克风/扬声器播放）。
    /// 阻塞调用，可能耗时数秒，插件应放后台线程执行。
    pub synthesize:
        unsafe extern "C" fn(ctx: VaHostCtx, text: *const c_char, out_err: *mut *mut c_char) -> i32,
    /// 切换「发送到麦克风」开关；out_json = {"mic_send":bool}（切换后的新状态）
    pub toggle_mic_send: unsafe extern "C" fn(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32,
    /// 播放最近一条消息（与「播放最近一条消息」快捷键同通道）
    pub play_last: unsafe extern "C" fn(ctx: VaHostCtx) -> i32,
    /// 状态查询：out_json = {"mic_send":bool,"playing_id":string|null,"synthesizing":bool}
    pub get_state: unsafe extern "C" fn(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32,
    /// 订阅宿主事件推送（收藏变化 / 设置变化 / 播放状态变化）。
    /// 事件 JSON 形如 {"type":"favorites_changed"|"settings_changed"|"playback_changed"}。
    /// 订阅在插件卸载前有效（dll 常驻，无需退订）。
    pub subscribe_events: unsafe extern "C" fn(
        ctx: VaHostCtx,
        cb: VaHostEventCallback,
        user_data: *mut std::os::raw::c_void,
    ) -> i32,
    /// 回写自身配置（仅 manifest 声明的 display 类型字段生效，用于上屏展示），
    /// 写入后宿主会刷新设置面板。
    pub set_own_config: unsafe extern "C" fn(
        ctx: VaHostCtx,
        key: *const c_char,
        value: *const c_char,
        out_err: *mut *mut c_char,
    ) -> i32,
    /// 通用用户确认弹窗（v2 新增）：宿主弹原生是/否对话框并阻塞等待用户选择。
    /// 返回：1=允许，0=拒绝，负值=错误（超时/不可用）。阻塞调用，插件应放后台线程。
    pub confirm_dialog: unsafe extern "C" fn(
        ctx: VaHostCtx,
        title: *const c_char,
        body: *const c_char,
    ) -> i32,
}

/// va_plugin_attach_host 签名：宿主加载插件后注入能力表。
/// services 指向宿主内存中的表，进程存活期间有效。
pub type VaPluginAttachHostFn = unsafe extern "C" fn(services: *const VaHostServices);

// ── 函数类型别名（宿主侧 libloading::Symbol<T> 用）──────────────

/// 元信息类：返回 NUL 结尾静态字符串指针（id/name/version/audio_format/list_voices 共用）
pub type VaStrFn = unsafe extern "C" fn() -> *const c_char;

/// 文本 → 音频。
/// text：NUL 结尾 UTF-8，必传；voice：NUL 结尾 UTF-8 或 NULL（NULL=插件默认音色）。
/// 成功返回 VA_OK 并写 out_audio/out_len；失败返回 VA_ERR，可选写 out_err。
pub type VaTtsSynthesizeFn = unsafe extern "C" fn(
    text: *const c_char,
    voice: *const c_char,
    out_audio: *mut *mut u8,
    out_len: *mut usize,
    out_err: *mut *mut c_char,
) -> i32;

/// 释放 va_tts_synthesize 输出的音频缓冲（ptr 与 len 必须原样传回）
pub type VaFreeBytesFn = unsafe extern "C" fn(ptr: *mut u8, len: usize);

/// 释放 va_tts_synthesize 输出的错误字符串
pub type VaFreeCstrFn = unsafe extern "C" fn(ptr: *mut c_char);

// ── ASR 函数类型别名 ────────────────────────────────────

/// 音频 → 文本（一次性转写）。
/// audio：裸 PCM/WAV 字节流指针；audio_len：字节长度；
/// language：NUL 结尾 UTF-8（如 "zh"、"en"），NULL = 插件自动检测。
/// 成功返回 VA_OK 并写 out_text（CString）；失败返回 VA_ERR，可选写 out_err。
/// out_text 内存由插件分配（CString::into_raw），宿主读取后调 va_free_cstr 归还。
pub type VaAsrTranscribeFn = unsafe extern "C" fn(
    audio: *const u8,
    audio_len: usize,
    language: *const c_char,
    out_text: *mut *mut c_char,
    out_err: *mut *mut c_char,
) -> i32;

/// 返回支持的语言列表 JSON（静态字符串，宿主立即拷贝，无需释放）。
/// 格式：[{"code":"zh","label":"中文"},{"code":"en","label":"English"}]
pub type VaAsrLanguagesFn = unsafe extern "C" fn() -> *const c_char;

// ── 可选：环境安装（setup）支持 ───────────────────────
//
// 本地引擎首次使用需要下载运行环境/模型。这两个符号可选导出
// （va_tts_plugin_setup! 宏生成），宿主 libloading 取不到就当插件无此能力。

/// 安装进度回调（宿主提供，插件在 setup 过程中反复调用）。
/// percent：0~100 定量进度；<0 表示不定量（此时以 message 文案为准）。
/// message：NUL 结尾 UTF-8 阶段描述（如"正在下载语音模型…"），指针仅在回调期间有效。
pub type VaSetupProgressFn =
    Option<unsafe extern "C" fn(percent: f32, message: *const c_char)>;

/// 查询环境安装状态：返回 JSON 字符串（内存约定同 va_list_voices：
/// 插件静态/缓存存储，宿主立即拷贝，无需释放）。
/// JSON 约定字段：ready(bool) / env_ready(bool) / resources_ready(bool) /
/// voices(已安装音色 id 数组) / summary(人类可读摘要)。
pub type VaPluginSetupStatusFn = unsafe extern "C" fn() -> *const c_char;

/// 执行环境安装/补齐。
/// options：NUL 结尾 UTF-8 JSON 或 NULL（如 {"voice":"mika"} 指定要确保的音色）；
/// progress：进度回调（可为 None）；out_msg：结束时写中文结果消息（CString，
/// 宿主读取后调 va_free_cstr 归还）。返回 VA_OK/VA_ERR。
pub type VaPluginSetupFn = unsafe extern "C" fn(
    options: *const c_char,
    progress: VaSetupProgressFn,
    out_msg: *mut *mut c_char,
) -> i32;

// ── 可选：音色管理（本地引擎的安装/卸载/预加载/导入音色包）──────
//
// 与 setup 符号同理：可选导出（va_tts_plugin_voices! 宏生成），
// 宿主 libloading 取不到就当插件无此能力。所有 out_msg 均为 CString，
// 宿主读取后调 va_free_cstr 归还；返回 VA_OK/VA_ERR。

/// 安装指定音色（预置角色首次会联网下载）。
/// voice_id：NUL 结尾 UTF-8，必传；progress：进度回调（可为 None，
/// 约定同 va_plugin_setup）。插件内部若发现运行环境未就绪会先补环境，
/// 进度文案应如实报告当前阶段。
pub type VaVoiceInstallFn = unsafe extern "C" fn(
    voice_id: *const c_char,
    progress: VaSetupProgressFn,
    out_msg: *mut *mut c_char,
) -> i32;

/// 卸载指定音色（删除本地音色包文件）。
pub type VaVoiceUninstallFn =
    unsafe extern "C" fn(voice_id: *const c_char, out_msg: *mut *mut c_char) -> i32;

/// 预加载已安装音色到内存（不触发下载；加载模型权重，秒级）。
pub type VaVoicePreloadFn =
    unsafe extern "C" fn(voice_id: *const c_char, out_msg: *mut *mut c_char) -> i32;

/// 导入用户自备音色包目录（插件校验布局后复制进自己的数据目录）。
/// src_dir：NUL 结尾 UTF-8 绝对路径。
pub type VaVoiceImportFn =
    unsafe extern "C" fn(src_dir: *const c_char, out_msg: *mut *mut c_char) -> i32;

// ── 音色条目 ──────────────────────────────────────────

/// va_list_voices 返回的 JSON 即 Vec<VoiceItem> 的序列化结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoiceItem {
    /// 音色 id（传给 va_tts_synthesize 的 voice 参数）
    pub id: String,
    /// 展示名（如 "晓晓（女·温暖）"）
    pub label: String,
}

/// 把音色列表序列化为插件用的 JSON 字符串（编译期之外的场景用，如动态音色表）
pub fn voices_to_json(voices: &[VoiceItem]) -> String {
    serde_json::to_string(voices).unwrap_or_else(|_| "[]".to_string())
}

// ── 插件侧导出宏 ──────────────────────────────────────

/// 插件侧一键生成全部 C ABI 导出函数。
///
/// 用法一：静态音色表（音色固定不变的引擎，如 edge-tts）：
/// ```ignore
/// plugin_api::va_tts_plugin! {
///     id: "edge-tts",
///     name: "Edge TTS（免费·微软）",
///     version: "1.0.0",
///     audio_format: "mp3",
///     voices_json: r#"[{"id":"zh-CN-XiaoxiaoNeural","label":"晓晓"}]"#,
///     synthesize: my_synthesize,   // fn(&str, Option<&str>) -> Result<Vec<u8>, String>
/// }
/// ```
///
/// 用法二：动态音色表（音色可运行期增减的引擎，如本地模型引擎用户自装音色包）：
/// ```ignore
/// plugin_api::va_tts_plugin! {
///     id: "my-local-tts",
///     name: "本地 TTS",
///     version: "1.0.0",
///     audio_format: "wav",
///     voices: list_voices,         // fn() -> Vec<plugin_api::VoiceItem>
///     synthesize: my_synthesize,
/// }
/// ```
///
/// - id/name/version/audio_format 必须是字符串字面量（生成 NUL 结尾静态串）；
/// - `voices_json` 是字符串字面量；`voices` 是 `fn() -> Vec<VoiceItem>`
///   （宿主每次查询音色表都会调用它，插件应保证该函数廉价且不 panic）；
/// - synthesize 是 `fn(&str, Option<&str>) -> Result<Vec<u8>, String>`
///   （文本、可选音色 → 音频字节 / 中文错误消息），内部如需异步请自建运行时 block_on；
/// - 宏内用 catch_unwind 包裹调用，插件 panic 不会跨 FFI 边界（否则未定义行为）。
#[macro_export]
macro_rules! va_tts_plugin {
    // ── 用法一：静态音色表 ──
    (
        id: $id:literal,
        name: $name:literal,
        version: $version:literal,
        audio_format: $fmt:literal,
        voices_json: $voices:literal,
        synthesize: $synth:expr $(,)?
    ) => {
        $crate::__va_tts_plugin_common! {
            id: $id,
            name: $name,
            version: $version,
            audio_format: $fmt,
            synthesize: $synth,
        }

        static VA__VOICES: &[u8] = concat!($voices, "\0").as_bytes();

        #[no_mangle]
        pub extern "C" fn va_list_voices() -> *const ::std::os::raw::c_char {
            VA__VOICES.as_ptr() as *const ::std::os::raw::c_char
        }
    };

    // ── 用法二：动态音色表 ──
    (
        id: $id:literal,
        name: $name:literal,
        version: $version:literal,
        audio_format: $fmt:literal,
        voices: $voices_fn:expr,
        synthesize: $synth:expr $(,)?
    ) => {
        $crate::__va_tts_plugin_common! {
            id: $id,
            name: $name,
            version: $version,
            audio_format: $fmt,
            synthesize: $synth,
        }

        #[no_mangle]
        pub extern "C" fn va_list_voices() -> *const ::std::os::raw::c_char {
            // 动态音色表缓存槽：每次调用重算并替换缓存，返回其指针。
            // 内存约定：宿主调用后立即拷贝，不长期持有指针，故旧缓存失效是安全的。
            static VA__VOICES_CACHE: ::std::sync::OnceLock<
                ::std::sync::Mutex<::std::ffi::CString>,
            > = ::std::sync::OnceLock::new();
            let cache = VA__VOICES_CACHE.get_or_init(|| {
                ::std::sync::Mutex::new(::std::ffi::CString::new("[]").unwrap())
            });
            let f: fn() -> ::std::vec::Vec<$crate::VoiceItem> = $voices_fn;
            // 音色函数异常时兜底空表，绝不 panic 跨 FFI 边界
            let voices = ::std::panic::catch_unwind(f).unwrap_or_default();
            let json = $crate::voices_to_json(&voices);
            let c = ::std::ffi::CString::new(json)
                .unwrap_or_else(|_| ::std::ffi::CString::new("[]").unwrap());
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            *guard = c;
            guard.as_ptr()
        }
    };
}

/// 内部宏：va_tts_plugin! 两个分支共用的导出函数
/// （id/name/version/audio_format + synthesize + 两个 free）。
#[doc(hidden)]
#[macro_export]
macro_rules! __va_tts_plugin_common {
    (
        id: $id:literal,
        name: $name:literal,
        version: $version:literal,
        audio_format: $fmt:literal,
        synthesize: $synth:expr $(,)?
    ) => {
        // NUL 结尾静态字节串（concat! 编译期拼接）
        static VA__ID: &[u8] = concat!($id, "\0").as_bytes();
        static VA__NAME: &[u8] = concat!($name, "\0").as_bytes();
        static VA__VERSION: &[u8] = concat!($version, "\0").as_bytes();
        static VA__FMT: &[u8] = concat!($fmt, "\0").as_bytes();

        #[no_mangle]
        pub extern "C" fn va_plugin_id() -> *const ::std::os::raw::c_char {
            VA__ID.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_plugin_name() -> *const ::std::os::raw::c_char {
            VA__NAME.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_plugin_version() -> *const ::std::os::raw::c_char {
            VA__VERSION.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_audio_format() -> *const ::std::os::raw::c_char {
            VA__FMT.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_tts_synthesize(
            text: *const ::std::os::raw::c_char,
            voice: *const ::std::os::raw::c_char,
            out_audio: *mut *mut u8,
            out_len: *mut usize,
            out_err: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            // 入参检查
            if text.is_null() || out_audio.is_null() || out_len.is_null() {
                return $crate::VA_ERR;
            }
            let text = match unsafe { ::std::ffi::CStr::from_ptr(text) }.to_str() {
                Ok(s) => s,
                Err(_) => return $crate::VA_ERR,
            };
            let voice: Option<&str> = if voice.is_null() {
                None
            } else {
                match unsafe { ::std::ffi::CStr::from_ptr(voice) }.to_str() {
                    Ok(s) => Some(s),
                    Err(_) => return $crate::VA_ERR,
                }
            };

            let synth: fn(&str, Option<&str>) -> Result<Vec<u8>, String> = $synth;
            // catch_unwind 兜底：panic 不跨 FFI 边界
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                synth(text, voice)
            }));

            match result {
                Ok(Ok(audio)) => {
                    // into_boxed_slice 保证容量==长度，释放侧可按 (ptr, len) 精确重建
                    let boxed = audio.into_boxed_slice();
                    let len = boxed.len();
                    let ptr = Box::into_raw(boxed) as *mut u8;
                    unsafe {
                        *out_audio = ptr;
                        *out_len = len;
                    }
                    $crate::VA_OK
                }
                Ok(Err(e)) => {
                    if !out_err.is_null() {
                        let c = ::std::ffi::CString::new(e)
                            .unwrap_or_else(|_| ::std::ffi::CString::new("unknown plugin error").unwrap());
                        unsafe { *out_err = c.into_raw(); }
                    }
                    $crate::VA_ERR
                }
                Err(_) => {
                    if !out_err.is_null() {
                        let c = ::std::ffi::CString::new("插件内部崩溃（panic）").unwrap();
                        unsafe { *out_err = c.into_raw(); }
                    }
                    $crate::VA_ERR
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn va_free_bytes(ptr: *mut u8, len: usize) {
            if !ptr.is_null() {
                unsafe {
                    let slice = ::std::slice::from_raw_parts_mut(ptr, len);
                    drop(Box::from_raw(slice));
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn va_free_cstr(ptr: *mut ::std::os::raw::c_char) {
            if !ptr.is_null() {
                unsafe { drop(::std::ffi::CString::from_raw(ptr)); }
            }
        }
    };
}


// ── ASR 插件导出宏 ──────────────────────────────────────

/// ASR 插件侧一键生成全部 C ABI 导出函数。
///
/// 用法：
/// ```ignore
/// plugin_api::va_asr_plugin! {
///     id: "whisper-asr",
///     name: "Whisper ASR（云端）",
///     version: "1.0.0",
///     languages: r#"[{"code":"zh","label":"中文"},{"code":"en","label":"English"}]"#,
///     transcribe: my_transcribe,  // fn(&[u8], Option<&str>) -> Result<String, String>
/// }
/// ```
///
/// - id/name/version 必须是字符串字面量；
/// - languages 是 JSON 字符串字面量（语言列表）；
/// - transcribe 是 `fn(&[u8], Option<&str>) -> Result<String, String>`
///   （音频字节、可选语言代码 → 转写文本 / 中文错误消息）。
#[macro_export]
macro_rules! va_asr_plugin {
    (
        id: $id:literal,
        name: $name:literal,
        version: $version:literal,
        languages: $langs:literal,
        transcribe: $transcribe:expr $(,)?
    ) => {
        // NUL 结尾静态字节串
        static VA__ID: &[u8] = concat!($id, "\0").as_bytes();
        static VA__NAME: &[u8] = concat!($name, "\0").as_bytes();
        static VA__VERSION: &[u8] = concat!($version, "\0").as_bytes();
        static VA__LANGS: &[u8] = concat!($langs, "\0").as_bytes();

        #[no_mangle]
        pub extern "C" fn va_plugin_id() -> *const ::std::os::raw::c_char {
            VA__ID.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_plugin_name() -> *const ::std::os::raw::c_char {
            VA__NAME.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_plugin_version() -> *const ::std::os::raw::c_char {
            VA__VERSION.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_asr_languages() -> *const ::std::os::raw::c_char {
            VA__LANGS.as_ptr() as *const ::std::os::raw::c_char
        }

        #[no_mangle]
        pub extern "C" fn va_asr_transcribe(
            audio: *const u8,
            audio_len: usize,
            language: *const ::std::os::raw::c_char,
            out_text: *mut *mut ::std::os::raw::c_char,
            out_err: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            if audio.is_null() || audio_len == 0 || out_text.is_null() {
                return $crate::VA_ERR;
            }
            let audio_slice = unsafe { ::std::slice::from_raw_parts(audio, audio_len) };
            let lang: Option<&str> = if language.is_null() {
                None
            } else {
                match unsafe { ::std::ffi::CStr::from_ptr(language) }.to_str() {
                    Ok(s) => Some(s),
                    Err(_) => return $crate::VA_ERR,
                }
            };

            let f: fn(&[u8], Option<&str>) -> Result<String, String> = $transcribe;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                f(audio_slice, lang)
            }));

            match result {
                Ok(Ok(text)) => {
                    let c = ::std::ffi::CString::new(text)
                        .unwrap_or_else(|_| ::std::ffi::CString::new("").unwrap());
                    unsafe { *out_text = c.into_raw(); }
                    $crate::VA_OK
                }
                Ok(Err(e)) => {
                    if !out_err.is_null() {
                        let c = ::std::ffi::CString::new(e)
                            .unwrap_or_else(|_| ::std::ffi::CString::new("unknown error").unwrap());
                        unsafe { *out_err = c.into_raw(); }
                    }
                    $crate::VA_ERR
                }
                Err(_) => {
                    if !out_err.is_null() {
                        let c = ::std::ffi::CString::new("ASR 插件内部崩溃（panic）").unwrap();
                        unsafe { *out_err = c.into_raw(); }
                    }
                    $crate::VA_ERR
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn va_free_cstr(ptr: *mut ::std::os::raw::c_char) {
            if !ptr.is_null() {
                unsafe { drop(::std::ffi::CString::from_raw(ptr)); }
            }
        }
    };
}

/// 插件侧可选导出：环境安装（setup）支持，与 va_tts_plugin! 配合使用。
///
/// 用法（插件 crate 的 lib.rs，va_tts_plugin! 之后）：
/// ```ignore
/// plugin_api::va_tts_plugin_setup! {
///     status: my_setup_status,  // fn() -> String（返回 JSON 状态）
///     setup: my_run_setup,      // fn(Option<&str>, &dyn Fn(f32, &str)) -> Result<String, String>
/// }
/// ```
///
/// - status：纯探测（磁盘检查），必须快、不触发网络，宿主列表页会频繁查询；
/// - setup：options 为调用方传入的 JSON（可为 None）；进度回调用 `cb(percent, msg)`
///   上报（percent<0 = 不定量）；Ok 消息直接展示给用户，Err 为中文错误；
/// - 宏负责 catch_unwind、CString 分配（由宿主的 va_free_cstr 归还），勿手写导出。
#[macro_export]
macro_rules! va_tts_plugin_setup {
    (
        status: $status_fn:expr,
        setup: $setup_fn:expr $(,)?
    ) => {
        #[no_mangle]
        pub extern "C" fn va_plugin_setup_status() -> *const ::std::os::raw::c_char {
            // 缓存槽约定同动态音色表：宿主调用后立即拷贝
            static VA__SETUP_STATUS_CACHE: ::std::sync::OnceLock<
                ::std::sync::Mutex<::std::ffi::CString>,
            > = ::std::sync::OnceLock::new();
            let cache = VA__SETUP_STATUS_CACHE.get_or_init(|| {
                ::std::sync::Mutex::new(::std::ffi::CString::new("{}").unwrap())
            });
            let f: fn() -> String = $status_fn;
            let json = ::std::panic::catch_unwind(f)
                .unwrap_or_else(|_| "{}".to_string());
            let c = ::std::ffi::CString::new(json)
                .unwrap_or_else(|_| ::std::ffi::CString::new("{}").unwrap());
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            *guard = c;
            guard.as_ptr()
        }

        #[no_mangle]
        pub unsafe extern "C" fn va_plugin_setup(
            options: *const ::std::os::raw::c_char,
            progress: $crate::VaSetupProgressFn,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            // 写入结果消息（CString::into_raw，宿主经 va_free_cstr 归还）
            macro_rules! write_msg {
                ($s:expr) => {
                    if !out_msg.is_null() {
                        let c = ::std::ffi::CString::new($s)
                            .unwrap_or_else(|_| ::std::ffi::CString::new("setup").unwrap());
                        *out_msg = c.into_raw();
                    }
                };
            }
            if out_msg.is_null() {
                return $crate::VA_ERR;
            }

            let opts: Option<&str> = if options.is_null() {
                None
            } else {
                match ::std::ffi::CStr::from_ptr(options).to_str() {
                    Ok(s) => Some(s),
                    Err(_) => {
                        write_msg!("安装参数不是合法 UTF-8".to_string());
                        return $crate::VA_ERR;
                    }
                }
            };

            // 把裸回调指针包成安全闭包（指针仅在 setup 调用期间有效，闭包不逃逸）
            let cb = |percent: f32, msg: &str| {
                if let Some(f) = progress {
                    let c = ::std::ffi::CString::new(msg)
                        .unwrap_or_else(|_| ::std::ffi::CString::new("").unwrap());
                    f(percent, c.as_ptr());
                }
            };

            let f: fn(Option<&str>, &dyn Fn(f32, &str)) -> Result<String, String> = $setup_fn;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                f(opts, &cb)
            }));

            match result {
                Ok(Ok(msg)) => {
                    write_msg!(msg);
                    $crate::VA_OK
                }
                Ok(Err(e)) => {
                    write_msg!(e);
                    $crate::VA_ERR
                }
                Err(_) => {
                    write_msg!("插件安装流程内部崩溃（panic）".to_string());
                    $crate::VA_ERR
                }
            }
        }
    };
}

/// 插件侧可选导出：音色管理支持（本地引擎的安装/卸载/预加载/导入音色包）。
///
/// 用法（插件 crate 的 lib.rs，va_tts_plugin! 之后）：
/// ```ignore
/// plugin_api::va_tts_plugin_voices! {
///     install: my_install_voice,      // fn(&str, &dyn Fn(f32, &str)) -> Result<String, String>
///     uninstall: my_uninstall_voice,  // fn(&str) -> Result<String, String>
///     preload: my_preload_voice,      // fn(&str) -> Result<String, String>
///     import: my_import_voice_pack,   // fn(&str) -> Result<String, String>（源目录绝对路径）
/// }
/// ```
///
/// - install：voice_id 必传；进度回调约定同 setup；若运行环境未就绪应先补环境
///   并在进度文案中如实报告；
/// - uninstall：删除本地音色包（插件自行决定目录布局）；
/// - preload：仅对已安装音色加载权重，不得触发下载；
/// - import：校验用户目录布局后复制进插件数据目录（保留用户原文件）；
/// - 四个函数 Ok 消息直接展示给用户，Err 为中文错误；宏负责 catch_unwind
///   与 CString 分配（宿主经 va_free_cstr 归还），勿手写导出。
#[macro_export]
macro_rules! va_tts_plugin_voices {
    (
        install: $install_fn:expr,
        uninstall: $uninstall_fn:expr,
        preload: $preload_fn:expr,
        import: $import_fn:expr $(,)?
    ) => {
        /// 内部共用：把 Result<String,String> 写入 out_msg 并返回 VA_OK/VA_ERR
        #[doc(hidden)]
        unsafe fn __va_voice_write_result(
            result: Result<String, String>,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            let (code, msg) = match result {
                Ok(m) => ($crate::VA_OK, m),
                Err(e) => ($crate::VA_ERR, e),
            };
            if !out_msg.is_null() {
                let c = ::std::ffi::CString::new(msg)
                    .unwrap_or_else(|_| ::std::ffi::CString::new("voice operation").unwrap());
                *out_msg = c.into_raw();
            }
            code
        }

        /// 内部共用：读 NUL 结尾 UTF-8 入参（NULL/非法 UTF-8 返回 Err 文案）
        #[doc(hidden)]
        unsafe fn __va_voice_read_cstr<'a>(
            ptr: *const ::std::os::raw::c_char,
            what: &str,
        ) -> Result<&'a str, String> {
            if ptr.is_null() {
                return Err(format!("{what}不能为空"));
            }
            ::std::ffi::CStr::from_ptr(ptr)
                .to_str()
                .map_err(|_| format!("{what}不是合法 UTF-8"))
        }

        #[no_mangle]
        pub unsafe extern "C" fn va_voice_install(
            voice_id: *const ::std::os::raw::c_char,
            progress: $crate::VaSetupProgressFn,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            let id = match unsafe { __va_voice_read_cstr(voice_id, "音色 id") } {
                Ok(s) => s,
                Err(e) => return __va_voice_write_result(Err(e), out_msg),
            };
            // 裸回调指针包成安全闭包（指针仅在本调用期间有效，闭包不逃逸）
            let cb = |percent: f32, msg: &str| {
                if let Some(f) = progress {
                    let c = ::std::ffi::CString::new(msg)
                        .unwrap_or_else(|_| ::std::ffi::CString::new("").unwrap());
                    f(percent, c.as_ptr());
                }
            };
            let f: fn(&str, &dyn Fn(f32, &str)) -> Result<String, String> = $install_fn;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                f(id, &cb)
            }));
            let flat = result.unwrap_or_else(|_| Err("插件音色安装流程内部崩溃（panic）".to_string()));
            __va_voice_write_result(flat, out_msg)
        }

        #[no_mangle]
        pub unsafe extern "C" fn va_voice_uninstall(
            voice_id: *const ::std::os::raw::c_char,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            let id = match unsafe { __va_voice_read_cstr(voice_id, "音色 id") } {
                Ok(s) => s,
                Err(e) => return __va_voice_write_result(Err(e), out_msg),
            };
            let f: fn(&str) -> Result<String, String> = $uninstall_fn;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| f(id)));
            let flat = result.unwrap_or_else(|_| Err("插件音色卸载流程内部崩溃（panic）".to_string()));
            __va_voice_write_result(flat, out_msg)
        }

        #[no_mangle]
        pub unsafe extern "C" fn va_voice_preload(
            voice_id: *const ::std::os::raw::c_char,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            let id = match unsafe { __va_voice_read_cstr(voice_id, "音色 id") } {
                Ok(s) => s,
                Err(e) => return __va_voice_write_result(Err(e), out_msg),
            };
            let f: fn(&str) -> Result<String, String> = $preload_fn;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| f(id)));
            let flat = result.unwrap_or_else(|_| Err("插件音色预加载流程内部崩溃（panic）".to_string()));
            __va_voice_write_result(flat, out_msg)
        }

        #[no_mangle]
        pub unsafe extern "C" fn va_voice_import(
            src_dir: *const ::std::os::raw::c_char,
            out_msg: *mut *mut ::std::os::raw::c_char,
        ) -> i32 {
            let src = match unsafe { __va_voice_read_cstr(src_dir, "音色包目录路径") } {
                Ok(s) => s,
                Err(e) => return __va_voice_write_result(Err(e), out_msg),
            };
            let f: fn(&str) -> Result<String, String> = $import_fn;
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| f(src)));
            let flat = result.unwrap_or_else(|_| Err("插件音色导入流程内部崩溃（panic）".to_string()));
            __va_voice_write_result(flat, out_msg)
        }
    };
}


// ── 插件侧宿主桥安全包装 ──────────────────────────────
//
// va_host_bridge! 宏把宿主注入的能力表存进本 dll 内的静态槽，
// host_bridge 模块在此之上提供带内存归属的 Rust 安全包装：
// 字符串自动拷出并归还、回调闭包自动装配，插件无需手写 FFI。

/// 宿主能力表静态槽（每个插件 dll 一份；宿主注入时经 va_host_bridge! 写入）
static HOST_SERVICES: OnceLock<HostServicesCell> = OnceLock::new();

struct HostServicesCell(*const VaHostServices);
// 宿主保证能力表进程存活期间有效，跨线程共享调用是安全的
unsafe impl Send for HostServicesCell {}
unsafe impl Sync for HostServicesCell {}

/// 宿主能力桥安全包装（插件侧使用，见 host_bridge 模块文档）
pub mod host_bridge {
    use super::*;

    /// 宿主能力桥是否已注入（attach 发生之前为 false）。
    pub fn available() -> bool {
        HOST_SERVICES.get().is_some()
    }

    /// 当前能力表（未注入时 None）。高级用法直接拿表调用。
    pub fn services() -> Option<&'static VaHostServices> {
        HOST_SERVICES.get().map(|cell| unsafe { &*cell.0 })
    }

    fn host() -> Result<&'static VaHostServices, String> {
        services().ok_or_else(|| "宿主能力桥未注入（宿主过旧或清单未声明 requires_host_bridge）".to_string())
    }

    /// 读宿主分配的 out JSON 字符串为 String 并归还内存
    fn take_json(code: i32, ptr: *mut c_char, svc: &VaHostServices) -> Result<String, String> {
        if code == VA_OK {
            if ptr.is_null() {
                return Ok(String::new());
            }
            let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
            unsafe { (svc.free_string)(svc.ctx, ptr) };
            Ok(s)
        } else {
            let msg = if ptr.is_null() {
                format!("宿主能力调用失败（错误码 {code}）")
            } else {
                let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
                unsafe { (svc.free_string)(svc.ctx, ptr) };
                s
            };
            Err(msg)
        }
    }

    fn take_err(code: i32, ptr: *mut c_char, svc: &VaHostServices) -> Result<(), String> {
        if code == VA_OK {
            if !ptr.is_null() {
                unsafe { (svc.free_string)(svc.ctx, ptr) };
            }
            Ok(())
        } else {
            let msg = if ptr.is_null() {
                format!("宿主能力调用失败（错误码 {code}）")
            } else {
                let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
                unsafe { (svc.free_string)(svc.ctx, ptr) };
                s
            };
            Err(msg)
        }
    }

    /// 收藏元数据 JSON：[{"id","note","created_at","hotkey"}]
    pub fn list_favorites_json() -> Result<String, String> {
        let svc = host()?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.list_favorites)(svc.ctx, &mut out) };
        take_json(code, out, svc)
    }

    /// 触发收藏播放（与收藏快捷键同逻辑）
    pub fn play_favorite(id: &str) -> Result<(), String> {
        let svc = host()?;
        let c_id = CString::new(id).map_err(|_| "收藏 id 含非法字符".to_string())?;
        let mut out_err: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.play_favorite)(svc.ctx, c_id.as_ptr(), &mut out_err) };
        take_err(code, out_err, svc)
    }

    /// 停止当前播放（扬声器 + 虚拟麦克风）
    pub fn stop_playback() -> Result<(), String> {
        let svc = host()?;
        let code = unsafe { (svc.stop_playback)(svc.ctx) };
        if code == VA_OK { Ok(()) } else { Err(format!("停止播放失败（错误码 {code}）")) }
    }

    /// 文字合成（走宿主现有合成管线）。阻塞调用，请在后台线程执行。
    pub fn synthesize(text: &str) -> Result<(), String> {
        let svc = host()?;
        let c_text = CString::new(text).map_err(|_| "文本含非法字符".to_string())?;
        let mut out_err: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.synthesize)(svc.ctx, c_text.as_ptr(), &mut out_err) };
        take_err(code, out_err, svc)
    }

    /// 切换「发送到麦克风」开关，返回切换后的新状态
    pub fn toggle_mic_send() -> Result<bool, String> {
        let svc = host()?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.toggle_mic_send)(svc.ctx, &mut out) };
        let json = take_json(code, out, svc)?;
        serde_json::from_str::<serde_json::Value>(&json)
            .ok()
            .and_then(|v| v.get("mic_send").and_then(|b| b.as_bool()))
            .ok_or_else(|| "宿主返回状态解析失败".to_string())
    }

    /// 播放最近一条消息
    pub fn play_last() -> Result<(), String> {
        let svc = host()?;
        let code = unsafe { (svc.play_last)(svc.ctx) };
        if code == VA_OK { Ok(()) } else { Err(format!("播放最近消息失败（错误码 {code}）")) }
    }

    /// 状态查询 JSON：{"mic_send","playing_id","synthesizing"}
    pub fn get_state_json() -> Result<String, String> {
        let svc = host()?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.get_state)(svc.ctx, &mut out) };
        take_json(code, out, svc)
    }

    /// 订阅宿主事件推送（收藏/设置/播放状态变化）。
    /// 回调可能在任意宿主线程触发，event 为 JSON 字符串（已拷贝，可自由使用）。
    pub fn subscribe_events<F>(f: F) -> Result<(), String>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let svc = host()?;
        // 闭包装箱成裸指针经 user_data 传回；订阅进程期内有效，不释放
        let user_data = Box::into_raw(Box::new(f)) as *mut std::os::raw::c_void;
        unsafe extern "C" fn trampoline<F>(user_data: *mut std::os::raw::c_void, event_json: *const c_char)
        where
            F: Fn(&str) + Send + Sync + 'static,
        {
            let f = &*(user_data as *const F);
            if event_json.is_null() {
                return;
            }
            let s = CStr::from_ptr(event_json).to_string_lossy();
            f(&s);
        }
        let cb: VaHostEventCallback = trampoline::<F>;
        let code = unsafe { (svc.subscribe_events)(svc.ctx, cb, user_data) };
        if code == VA_OK { Ok(()) } else { Err(format!("订阅宿主事件失败（错误码 {code}）")) }
    }

    /// 回写自身配置（仅 display 类型字段生效）
    pub fn set_own_config(key: &str, value: &str) -> Result<(), String> {
        let svc = host()?;
        let c_key = CString::new(key).map_err(|_| "key 含非法字符".to_string())?;
        let c_val = CString::new(value).map_err(|_| "value 含非法字符".to_string())?;
        let mut out_err: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (svc.set_own_config)(svc.ctx, c_key.as_ptr(), c_val.as_ptr(), &mut out_err) };
        take_err(code, out_err, svc)
    }

    /// 通用用户确认弹窗（v2）：阻塞等待用户在宿主弹窗上点是/否。
    /// Ok(true)=允许，Ok(false)=拒绝，Err=超时或能力不可用。
    pub fn confirm_dialog(title: &str, body: &str) -> Result<bool, String> {
        let svc = host()?;
        let c_title = CString::new(title).map_err(|_| "标题含非法字符".to_string())?;
        let c_body = CString::new(body).map_err(|_| "内容含非法字符".to_string())?;
        let code = unsafe { (svc.confirm_dialog)(svc.ctx, c_title.as_ptr(), c_body.as_ptr()) };
        match code {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err("确认弹窗超时或不可用".to_string()),
        }
    }

    /// 宿主能力表存入静态槽（va_host_bridge! 宏内部使用，插件勿直接调用）
    #[doc(hidden)]
    pub fn __store_host_services(services: *const VaHostServices) {
        let _ = HOST_SERVICES.set(HostServicesCell(services));
    }
}

/// 插件侧可选导出：宿主能力桥接入点。宿主取到本符号即注入能力表。
///
/// 用法（插件 crate 的 lib.rs）：
/// ```ignore
/// plugin_api::va_host_bridge! {
///     on_attach: my_init,   // fn(&plugin_api::VaHostServices)，可为占位 | _| {}
/// }
/// ```
///
/// - 宏导出 `va_plugin_attach_host`：存表 → 调 on_attach（catch_unwind 包裹，
///   panic 不跨 FFI 边界）；
/// - 之后插件任意线程用 `plugin_api::host_bridge::*` 安全包装调用宿主能力；
/// - 未被宿主注入时 host_bridge 调用返回 Err，插件应优雅降级。
#[macro_export]
macro_rules! va_host_bridge {
    (on_attach: $on_attach:expr $(,)?) => {
        #[no_mangle]
        pub unsafe extern "C" fn va_plugin_attach_host(services: *const $crate::VaHostServices) {
            if services.is_null() {
                return;
            }
            if unsafe { &*services }.version < $crate::VA_HOST_SERVICES_VERSION {
                return; // 宿主能力表版本过低，不接入（保持未注入状态）
            }
            $crate::host_bridge::__store_host_services(services);
            let f: fn(&$crate::VaHostServices) = $on_attach;
            let svc = unsafe { &*services };
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| f(svc)));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_item_序列化格式() {
        let v = vec![VoiceItem { id: "a".into(), label: "甲".into() }];
        let json = voices_to_json(&v);
        assert_eq!(json, r#"[{"id":"a","label":"甲"}]"#);
    }

    #[test]
    fn voice_item_反序列化() {
        let json = r#"[{"id":"x","label":"X"},{"id":"y","label":"Y"}]"#;
        let list: Vec<VoiceItem> = serde_json::from_str(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].id, "y");
    }

    #[test]
    fn 能力表为空指针安全包装返回错误() {
        // 未注入时（本测试进程从未 attach），安全包装应返回 Err 而非 panic
        assert!(!host_bridge::available());
        assert!(host_bridge::get_state_json().is_err());
        assert!(host_bridge::list_favorites_json().is_err());
        assert!(host_bridge::set_own_config("k", "v").is_err());
    }

    #[test]
    fn 能力表字段布局_c_abi_约定() {
        // 宿主与插件各自编译本 crate，repr(C) 布局必须一致：
        // 校验关键字段偏移符合预期（u32 version 前置 + 指针对齐填充）
        assert_eq!(std::mem::offset_of!(VaHostServices, version), 0);
        assert_eq!(
            std::mem::offset_of!(VaHostServices, ctx),
            std::mem::size_of::<usize>(),
            "ctx 应紧跟 version 之后的指针对齐位置"
        );
        // 12 个成员：version(+填充) + ctx + 10 个函数指针
        assert_eq!(std::mem::size_of::<VaHostServices>(), (2 + 10) * std::mem::size_of::<usize>());
    }
}
