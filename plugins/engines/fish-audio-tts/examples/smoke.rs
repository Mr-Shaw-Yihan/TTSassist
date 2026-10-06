// fish-audio-tts FFI 冒烟测试：以宿主完全相同的方式（libloading 按符号名取函数）
// 加载本插件的 release dll，端到端验证 C ABI 导出、元信息、真实 API 合成与内存归还。
//
// 前置（在插件目录 plugins/engines/fish-audio-tts 下执行）：
//   1. cargo build --release -p fish-audio-tts
//   2. $env:FISH_AUDIO_API_KEY = "sk-fish-..."        # 真实 Key
//   3. cargo run --release -p fish-audio-tts --example smoke -- ..\..\target\release\fish_audio_tts.dll
//
// 输出：fish_smoke_default.mp3（官方默认音色）+ fish_smoke_voice.mp3（指定音色）

use libloading::{Library, Symbol};
use std::ffi::{c_char, CStr, CString};
use std::time::Instant;

type StrFn = unsafe extern "C" fn() -> *const c_char;
type SynthFn = unsafe extern "C" fn(
    text: *const c_char,
    voice: *const c_char,
    out_audio: *mut *mut u8,
    out_len: *mut usize,
    out_err: *mut *mut c_char,
) -> i32;
type FreeBytesFn = unsafe extern "C" fn(ptr: *mut u8, len: usize);
type FreeCstrFn = unsafe extern "C" fn(ptr: *mut c_char);

fn main() {
    let dll_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"..\..\target\release\fish_audio_tts.dll".to_string());
    println!("加载 dll: {dll_path}");
    let lib = unsafe { Library::new(&dll_path) }.expect("dll 加载失败（先 cargo build --release -p fish-audio-tts）");

    // ── 元信息符号 ──
    let (id, name, version, fmt, voices) = unsafe {
        (
            lib.get::<StrFn>(b"va_plugin_id\0").unwrap(),
            lib.get::<StrFn>(b"va_plugin_name\0").unwrap(),
            lib.get::<StrFn>(b"va_plugin_version\0").unwrap(),
            lib.get::<StrFn>(b"va_audio_format\0").unwrap(),
            lib.get::<StrFn>(b"va_list_voices\0").unwrap(),
        )
    };
    let (id, name, version, fmt) = (
        cstr(&id),
        cstr(&name),
        cstr(&version),
        cstr(&fmt),
    );
    println!("id={id} name={name} version={version} audio_format={fmt}");
    assert_eq!(id, "fish-audio-tts", "dll 自报 id 必须与 manifest.id 一致");

    let voices_json = cstr(&voices);
    let voice_count = voices_json.matches("\"id\"").count();
    println!("音色表条目数: {voice_count}");
    assert!(voice_count >= 14, "音色表应包含 default + 13 个社区音色");

    let synth: Symbol<SynthFn> = unsafe { lib.get(b"va_tts_synthesize\0") }.unwrap();
    let free_bytes: Symbol<FreeBytesFn> = unsafe { lib.get(b"va_free_bytes\0") }.unwrap();
    let free_cstr: Symbol<FreeCstrFn> = unsafe { lib.get(b"va_free_cstr\0") }.unwrap();

    // ── 合成 1：默认音色（voice = NULL）──
    let text1 = CString::new("你好，这是电子声带通过 Fish Audio 插件合成的语音。").unwrap();
    let mut audio: *mut u8 = std::ptr::null_mut();
    let mut len: usize = 0;
    let mut err: *mut c_char = std::ptr::null_mut();
    let t0 = Instant::now();
    let code = unsafe { synth(text1.as_ptr(), std::ptr::null(), &mut audio, &mut len, &mut err) };
    report("默认音色", t0, code, audio, len, err, "fish_smoke_default.mp3", &free_bytes, &free_cstr);

    // ── 合成 2：指定音色（央视配音，验证 reference_id 路径）──
    let text2 = CString::new("今天天气不错，适合出门散步。").unwrap();
    let voice = CString::new("59cb5986671546eaa6ca8ae6f29f6d22").unwrap();
    let mut audio2: *mut u8 = std::ptr::null_mut();
    let mut len2: usize = 0;
    let mut err2: *mut c_char = std::ptr::null_mut();
    let t1 = Instant::now();
    let code2 = unsafe { synth(text2.as_ptr(), voice.as_ptr(), &mut audio2, &mut len2, &mut err2) };
    report("指定音色", t1, code2, audio2, len2, err2, "fish_smoke_voice.mp3", &free_bytes, &free_cstr);

    println!("\nFFI 冒烟全部通过 ✓");
}

fn cstr(f: &Symbol<StrFn>) -> String {
    unsafe { CStr::from_ptr(f()) }.to_string_lossy().into_owned()
}

/// 打印合成结果、写 mp3、归还插件内存（ptr/len 原样传回）
fn report(
    label: &str,
    t0: Instant,
    code: i32,
    audio: *mut u8,
    len: usize,
    err: *mut c_char,
    out_file: &str,
    free_bytes: &Symbol<FreeBytesFn>,
    free_cstr: &Symbol<FreeCstrFn>,
) {
    if code != 0 {
        let msg = if err.is_null() {
            "（无错误详情）".to_string()
        } else {
            let s = unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned();
            unsafe { free_cstr(err) };
            s
        };
        panic!("{label} 合成失败（VA_ERR）：{msg}");
    }
    assert!(!audio.is_null() && len > 0, "{label} 返回 VA_OK 但音频为空");
    let bytes = unsafe { std::slice::from_raw_parts(audio, len) };
    // MP3 帧同步字：0xFFEx / 0xFFFx（buffered 全量响应必以帧头开头）
    assert!(bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0, "{label} 输出不是 MP3 帧头");
    std::fs::write(out_file, bytes).expect("写 mp3 失败");
    unsafe { free_bytes(audio, len) };
    println!("{label}: OK，{} 字节，耗时 {:.2}s → {out_file}", len, t0.elapsed().as_secs_f64());
}
