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

// ── ASR 插件导出符号 ──────────────────────────────────────
pub const SYM_ASR_TRANSCRIBE: &[u8] = b"va_asr_transcribe\0";
pub const SYM_ASR_LANGUAGES: &[u8] = b"va_asr_languages\0";

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
