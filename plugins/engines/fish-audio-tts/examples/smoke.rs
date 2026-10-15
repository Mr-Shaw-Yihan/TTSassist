// fish-audio-tts FFI 冒烟测试：以宿主完全相同的方式（libloading 按符号名取函数）
// 加载本插件的 release dll，端到端验证 C ABI 导出、元信息、真实 API 合成
// （阻塞 MP3 + 流式 PCM 分块）与内存归还。
//
// 前置（在插件目录 plugins/engines/fish-audio-tts 下执行）：
//   1. cargo build --release -p fish-audio-tts
//   2. $env:FISH_AUDIO_API_KEY = "sk-fish-..."        # 真实 Key
//   3. cargo run --release -p fish-audio-tts --example smoke -- ..\..\target\release\fish_audio_tts.dll
//
// 输出：fish_smoke_default.mp3（默认音色阻塞）+ fish_smoke_voice.mp3（指定音色）
//       + fish_smoke_stream.wav（流式收全后转 WAV，可试听首响对比）

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

// 流式三件套（宿主同款签名；会话句柄对宿主不透明）
type StreamStartFn = unsafe extern "C" fn(
    text: *const c_char,
    voice: *const c_char,
    out_session: *mut *mut std::os::raw::c_void,
    out_info: *mut *mut c_char,
    out_err: *mut *mut c_char,
) -> i32;
type StreamNextFn = unsafe extern "C" fn(
    session: *mut std::os::raw::c_void,
    out_chunk: *mut *mut u8,
    out_len: *mut usize,
    out_err: *mut *mut c_char,
) -> i32;
type StreamStopFn = unsafe extern "C" fn(session: *mut std::os::raw::c_void);

fn main() {
    let dll_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"..\..\target\release\fish_audio_tts.dll".to_string());
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
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
    let (id, name, version, fmt) = (cstr(&id), cstr(&name), cstr(&version), cstr(&fmt));
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("id={id} name={name} version={version} audio_format={fmt}");
    assert_eq!(id, "fish-audio-tts", "dll 自报 id 必须与 manifest.id 一致");

    let voices_json = cstr(&voices);
    let voice_count = voices_json.matches("\"id\"").count();
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("音色表条目数: {voice_count}");
    assert!(voice_count >= 14, "音色表应包含 default + 13 个社区音色");

    let synth: Symbol<SynthFn> = unsafe { lib.get(b"va_tts_synthesize\0") }.unwrap();
    let free_bytes: Symbol<FreeBytesFn> = unsafe { lib.get(b"va_free_bytes\0") }.unwrap();
    let free_cstr: Symbol<FreeCstrFn> = unsafe { lib.get(b"va_free_cstr\0") }.unwrap();

    // ── 阻塞合成 1：默认音色（voice = NULL）──
    let text1 = CString::new("你好，这是电子声带通过 Fish Audio 插件合成的语音。").unwrap();
    let mut audio: *mut u8 = std::ptr::null_mut();
    let mut len: usize = 0;
    let mut err: *mut c_char = std::ptr::null_mut();
    let t0 = Instant::now();
    let code = unsafe { synth(text1.as_ptr(), std::ptr::null(), &mut audio, &mut len, &mut err) };
    report_blocking("阻塞·默认音色", t0, code, audio, len, err, "fish_smoke_default.mp3", &free_bytes, &free_cstr);

    // ── 阻塞合成 2：指定音色（央视配音，验证 reference_id 路径）──
    let text2 = CString::new("今天天气不错，适合出门散步。").unwrap();
    let voice = CString::new("59cb5986671546eaa6ca8ae6f29f6d22").unwrap();
    let mut audio2: *mut u8 = std::ptr::null_mut();
    let mut len2: usize = 0;
    let mut err2: *mut c_char = std::ptr::null_mut();
    let t1 = Instant::now();
    let code2 = unsafe { synth(text2.as_ptr(), voice.as_ptr(), &mut audio2, &mut len2, &mut err2) };
    report_blocking("阻塞·指定音色", t1, code2, audio2, len2, err2, "fish_smoke_voice.mp3", &free_bytes, &free_cstr);

    // ── 流式能力符号必须齐全（宿主判定口径：三个都有才算流式）──
    let stream_start = unsafe { lib.get::<StreamStartFn>(b"va_tts_stream_start\0") }.expect("缺 va_tts_stream_start");
    let stream_next = unsafe { lib.get::<StreamNextFn>(b"va_tts_stream_next\0") }.expect("缺 va_tts_stream_next");
    let stream_stop = unsafe { lib.get::<StreamStopFn>(b"va_tts_stream_stop\0") }.expect("缺 va_tts_stream_stop");

    // ── 流式合成：长文本验证分块陆续到达 + 首响 ──
    let long_text = CString::new(
        "语音合成技术正在快速发展。今天我们来测试流式输出能力，观察音频数据块是如何陆续到达宿主并即时播放的。\
         第一个数据块很快就能听到，这正是边合成边播放方案的价值。第二段内容继续补充，用来拉长文本，\
         让服务端必须分成多个音频块下发。最后一句收尾。",
    )
    .unwrap();
    let mut session: *mut std::os::raw::c_void = std::ptr::null_mut();
    let mut info: *mut c_char = std::ptr::null_mut();
    let mut serr: *mut c_char = std::ptr::null_mut();
    let tstart = Instant::now();
    let scode = unsafe { stream_start(long_text.as_ptr(), std::ptr::null(), &mut session, &mut info, &mut serr) };
    if scode != 0 {
        let msg = cstr_from(&mut serr, &free_cstr);
        panic!("stream_start 失败: {msg}");
    }
    let info_s = unsafe { CStr::from_ptr(info) }.to_string_lossy().into_owned();
    unsafe { free_cstr(info) };
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("stream_start 会话打开: info={info_s}（{:.2}s）", tstart.elapsed().as_secs_f64());
    assert!(info_s.contains("pcm_s16le") && info_s.contains("44100") && info_s.contains("\"channels\":1"), "info 必须声明宿主支持的 pcm_s16le 44.1k 单声道");

    let mut chunks: Vec<u8> = Vec::new();
    let mut first_chunk_at: Option<f64> = None;
    let mut n_chunks = 0usize;
    loop {
        let mut cbuf: *mut u8 = std::ptr::null_mut();
        let mut clen: usize = 0;
        let mut cerr: *mut c_char = std::ptr::null_mut();
        let rcode = unsafe { stream_next(session, &mut cbuf, &mut clen, &mut cerr) };
        if rcode != 0 {
            let msg = cstr_from(&mut cerr, &free_cstr);
            panic!("stream_next 失败: {msg}");
        }
        if clen == 0 {
            if !cbuf.is_null() {
                unsafe { free_bytes(cbuf, clen) };
            }
            break; // 流正常结束
        }
        assert_eq!(clen % 2, 0, "PCM 块必须偶数字节");
        let slice = unsafe { std::slice::from_raw_parts(cbuf, clen) };
        if first_chunk_at.is_none() {
            first_chunk_at = Some(tstart.elapsed().as_secs_f64());
        }
        chunks.extend_from_slice(slice);
        n_chunks += 1;
        unsafe { free_bytes(cbuf, clen) };
        if n_chunks == 1 || n_chunks % 8 == 0 {
            // allowlog: 示例 smoke CLI 进度输出，不含用户文本
            println!("  块#{n_chunks} {clen}B @ {:.2}s", tstart.elapsed().as_secs_f64());
        }
    }
    let total_t = tstart.elapsed().as_secs_f64();
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!(
        "流式: {} 块共 {}B，首块 @ {:.2}s（总 {:.2}s）→ fish_smoke_stream.wav",
        n_chunks,
        chunks.len(),
        first_chunk_at.unwrap_or(total_t),
        total_t
    );
    assert!(n_chunks > 1, "长文本应收到多个分块（实测仅 {} 块）", n_chunks);
    let dur = chunks.len() as f64 / (44100.0 * 2.0);
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("流式音频时长（s16le/44.1k/mono 解释）≈ {dur:.1}s");
    std::fs::write("fish_smoke_stream.wav", wav_wrap(&chunks, 44100, 1)).expect("写 wav 失败");

    // ── 流式中止语义：再开一个会话立即 stop，next 应亚秒级返回错误 ──
    let mut session2: *mut std::os::raw::c_void = std::ptr::null_mut();
    let mut info2: *mut c_char = std::ptr::null_mut();
    let mut serr2: *mut c_char = std::ptr::null_mut();
    let scode2 = unsafe { stream_start(long_text.as_ptr(), std::ptr::null(), &mut session2, &mut info2, &mut serr2) };
    if scode2 != 0 {
        let msg = cstr_from(&mut serr2, &free_cstr);
        panic!("stream_start(中止测试) 失败: {msg}");
    }
    unsafe { free_cstr(info2) };
    unsafe { stream_stop(session2) };
    let tab = Instant::now();
    let mut cbuf: *mut u8 = std::ptr::null_mut();
    let mut clen: usize = 0;
    let mut cerr: *mut c_char = std::ptr::null_mut();
    let rcode2 = unsafe { stream_next(session2, &mut cbuf, &mut clen, &mut cerr) };
    let wake = tab.elapsed();
    let msg2 = if cerr.is_null() { String::new() } else { cstr_from(&mut cerr, &free_cstr) };
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("abort 后 next 返回 code={rcode2}「{msg2}」，耗时 {:.3}s", wake.as_secs_f64());
    assert_eq!(rcode2, 1, "abort 后 next 必须返回 VA_ERR");
    assert!(wake < std::time::Duration::from_secs(1), "abort 后 next 必须亚秒级返回");
    if !cbuf.is_null() {
        unsafe { free_bytes(cbuf, clen) };
    }

    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("\nFFI 冒烟全部通过 ✓（阻塞 ×2 + 流式收全 + 中止语义）");
}

fn cstr(f: &Symbol<StrFn>) -> String {
    unsafe { CStr::from_ptr(f()) }.to_string_lossy().into_owned()
}

fn cstr_from(p: &mut *mut c_char, free: &Symbol<FreeCstrFn>) -> String {
    if p.is_null() {
        return "（无错误详情）".to_string();
    }
    let s = unsafe { CStr::from_ptr(*p) }.to_string_lossy().into_owned();
    unsafe { free(*p) };
    *p = std::ptr::null_mut();
    s
}

/// 打印合成结果、写 mp3、归还插件内存（ptr/len 原样传回）
fn report_blocking(
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
        let msg = cstr_from(&mut { err }, free_cstr);
        panic!("{label} 合成失败（VA_ERR）：{msg}");
    }
    assert!(!audio.is_null() && len > 0, "{label} 返回 VA_OK 但音频为空");
    let bytes = unsafe { std::slice::from_raw_parts(audio, len) };
    // MP3 帧同步字：0xFFEx / 0xFFFx（buffered 全量响应必以帧头开头）
    assert!(bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0, "{label} 输出不是 MP3 帧头");
    std::fs::write(out_file, bytes).expect("写 mp3 失败");
    unsafe { free_bytes(audio, len) };
    // allowlog: 示例 smoke CLI 进度输出，不含用户文本
    println!("{label}: OK，{} 字节，耗时 {:.2}s → {out_file}", len, t0.elapsed().as_secs_f64());
}

/// PCM s16le → WAV（44 字节标准头，便于直接试听）
fn wav_wrap(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let byte_rate = sample_rate * channels as u32 * 2;
    let mut w: Vec<u8> = Vec::with_capacity(44 + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&byte_rate.to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend_from_slice(pcm);
    w
}
