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

use std::ffi::c_char;

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

// 可选符号（本地引擎"环境安装"支持；老插件没有，宿主按 Option 处理）
pub const SYM_PLUGIN_SETUP_STATUS: &[u8] = b"va_plugin_setup_status\0";
pub const SYM_PLUGIN_SETUP: &[u8] = b"va_plugin_setup\0";

// 可选符号（本地引擎"音色管理"支持；va_tts_plugin_voices! 宏生成，老插件没有）
pub const SYM_VOICE_INSTALL: &[u8] = b"va_voice_install\0";
pub const SYM_VOICE_UNINSTALL: &[u8] = b"va_voice_uninstall\0";
pub const SYM_VOICE_PRELOAD: &[u8] = b"va_voice_preload\0";
pub const SYM_VOICE_IMPORT: &[u8] = b"va_voice_import\0";

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
}
