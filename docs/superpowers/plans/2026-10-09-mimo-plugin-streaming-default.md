# MiMo 插件化与流式默认 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把内置的 MiMo TTS 引擎下沉为「自带且可卸载」的 `mimo-tts` 插件（只走流式、零降级代码），新用户默认引擎改为免 Key 的 `edge-tts`，并把「本体只做基座」立成可执行护栏。

**Architecture:** 插件侧用 `plugin-api` 的双导出（`va_tts_plugin!` 阻塞兼容面 + `va_tts_stream_plugin!` 流式主面），内部 worker 线程用 `reqwest::blocking` 读 MiMo SSE 并把块塞进 `mpsc`，宿主按拉模型逐块边收边播；宿主侧删掉所有 MiMo 身份分支，差异一律靠 manifest / voices 自述；克隆音色的"低延迟未上线"由服务端自动退化为兼容模式，客户端不写降级分支。

**Tech Stack:** Rust 2021 / cdylib 插件 + C ABI / `reqwest 0.12 (blocking, rustls-tls)` / Tauri v2（Rust 事件 `emit` + `listen_any`）/ React + TypeScript + Zustand / PowerShell 7 护栏脚本 / GitHub Actions `consistency.yml`。

**设计依据：** `doc/MiMo插件化与流式默认设计.md`（v3，已批准）。本计划逐条覆盖其 §3.1 / §3.2 / §3.4 / §3.5 / §3.6 / §四 / §五 / §六 / §八。

---

## 执行须知（每个 Task 都适用）

**工作树**：`d:\Docs\Project\TTKook\TTKook-mimo-plugin`（分支 `feat/mimo-plugin`，基线 `main@dc55a57`）。
`doc/` 在仓库外，**不要** `git add` 任何 `doc/` 下的文件。

**每条 cargo 命令前必须先设这两个环境变量**（本机曾有其它会话把全局 `CARGO_TARGET_DIR` 指到隔壁工作树，导致构建产物污染别人的 exe）：

```powershell
$env:CARGO_HTTP_CHECK_REVOKE = "false"
# 宿主（src-tauri）用：
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
# 插件 workspace（plugins）用：
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
```

同一条 shell 里会累积设置，切 workspace 时**重新赋值**再跑，别混用。

**git 纪律**：逐文件 `git add <路径>`，禁止 `git add -A` / `git add .`；commit message 用简体中文并在结尾带
`Co-Authored-By: Claude <noreply@anthropic.com>`。计划里每个 commit 步骤已给出 add 清单，照抄即可。

**基线**（Task 1 开工前应为绿）：

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib
```
预期：`170 passed; 0 failed; 4 ignored`。

---

## 设计 §八 七步 → 本计划 Task 映射

> **接手的模型/人先去看文件末尾前的「执行交接日志」一节**（搜 `## 执行交接日志`）：
> 那里记着已完成到哪个 Task、下一个动作、以及容易错的三条环境/提交约束。

| 设计步骤 | Task |
|---|---|
| 1 插件 crate（含 RIFF 前置实测） | Task 1 / 2 / 3 / 4 |
| 2 mimo-asr 补 config 重打包（硬性配对） | Task 5（与宿主删注入同提交） |
| 3 宿主删内置 MiMo + 迁移 + 默认引擎 + 卸载兜底 + `VA_CLONE_SAMPLE` | Task 6 / 7 / 8 |
| 4 流式播放态事件 + 桥路径走流式 | Task 9 |
| 5 前端 + help_url 覆盖 + 类型测试同步 | Task 10 / 11 |
| 6 基座护栏 + AGENTS.md | Task 12 |
| 7 探针实网 + 门禁 + 文档回写 | Task 4（RIFF 提前）/ Task 13 |

---

## Task 1：插件协议层 `mimo-tts::client`（纯函数，无网络）

**Files:**
- Create: `plugins/engines/mimo-tts/Cargo.toml`
- Create: `plugins/engines/mimo-tts/src/lib.rs`（本 Task 只放空壳，Task 2 补导出）
- Create: `plugins/engines/mimo-tts/src/client.rs`
- Modify: `plugins/Cargo.lock`（cargo 自动）

- [ ] **Step 1：建 crate 骨架**

`plugins/engines/mimo-tts/Cargo.toml`：

```toml
[package]
name = "mimo-tts"
version = "0.1.0"
description = "MiMo TTS 插件：小米 MiMo-V2.5-TTS 云端合成（流式优先，支持音色克隆）"
edition = "2021"

[lib]
# 插件必须是动态库（Windows 上产出 mimo_tts.dll，打包时改名 plugin.dll）
crate-type = ["cdylib"]

[dependencies]
plugin-api = { path = "../../plugin-api" }
# MiMo 是 HTTP SSE：用 blocking 客户端在 worker 线程里逐行读，
# 不依赖宿主 tokio 运行时（存档分支用的是 async + futures_util，cdylib 内不宜）
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"

[dev-dependencies]
# 探针把流式 PCM 落 wav 供试听（仅测试用，不进插件产物）
hound = "3.5"
```

`plugins/engines/mimo-tts/src/lib.rs`（空壳，Task 2 替换）：

```rust
// MiMo TTS 插件（小米·云端·流式）。协议层见 client.rs，流式会话见 stream.rs。
mod client;
mod stream;
```

`plugins/engines/mimo-tts/src/client.rs` 与 `src/stream.rs` 先各写一行占位注释
（`// 实现见 Task 1 / Task 2`），保证能编译。

- [ ] **Step 2：跑空测试确认 crate 已进 workspace**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo test -p mimo-tts
```
预期：编译通过（首次会拉 reqwest/hound 依赖，耗时数分钟），`0 tests`。
（写本计划时误猜 `hound = "5"`，crates.io 上 hound 最高只到 3.5.1，已改为 `"3.5"`；
执行时若又碰到依赖版本猜错，以 `cargo` 报的 candidate versions 为准就地改掉。）
`plugins/Cargo.lock` 出现 `mimo-tts` 条目（workspace 的 `members = ["engines/*"]` glob 自动收录，无需改 `plugins/Cargo.toml`）。

- [ ] **Step 3：写失败测试（请求体 / SSE / WAV / 对齐 / 克隆）**

`plugins/engines/mimo-tts/src/client.rs` 末尾追加（先只写测试，函数还不存在）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 造一行 SSE：data: {...delta.audio.data: base64...}
    fn data_line(bytes: &[u8]) -> String {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "audio": { "data": b64 } } }] })
        )
    }

    #[test]
    fn 请求体_文本放assistant() {
        let body = build_request_body("mimo-v2.5-tts", "你好世界", Some("冰糖"), None);
        assert_eq!(body["model"], "mimo-v2.5-tts");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][1]["content"], "你好世界");
        assert_eq!(body["audio"]["voice"], "冰糖");
        assert_eq!(body["audio"]["format"], "wav");
    }

    #[test]
    fn 请求体_缺省音色与指令() {
        let body = build_request_body("mimo-v2.5-tts", "x", None, None);
        assert_eq!(body["audio"]["voice"], DEFAULT_VOICE);
        assert_eq!(body["messages"][0]["content"], "");
    }

    #[test]
    fn 流式请求体_只改两处() {
        let plain = build_request_body("mimo-v2.5-tts", "你好", Some("冰糖"), None);
        let stream = build_stream_request_body("mimo-v2.5-tts", "你好", Some("冰糖"), None);
        assert_eq!(stream["stream"], true);
        assert_eq!(stream["audio"]["format"], "pcm16");
        // 其余字段必须与非流式一致（防两条路径参数漂移）
        assert_eq!(stream["messages"], plain["messages"]);
        assert_eq!(stream["model"], plain["model"]);
    }

    #[test]
    fn 非流式响应_取base64wav() {
        use base64::Engine as _;
        let fake = b"RIFFxxxxWAVEfake_wav_data";
        let resp = serde_json::json!({
            "choices": [{ "message": { "audio": { "data": base64::engine::general_purpose::STANDARD.encode(fake) } } }]
        });
        assert_eq!(extract_audio_data(&resp).unwrap(), fake.to_vec());
    }

    #[test]
    fn 非流式响应_缺字段给中文错() {
        let err = extract_audio_data(&serde_json::json!({ "choices": [] })).unwrap_err();
        assert!(err.contains("响应"), "实际: {err}");
    }

    #[test]
    fn sse_完整单行取出裸pcm() {
        let line = data_line(&[1, 2, 3, 4]);
        assert_eq!(sse_line_to_pcm(&line).unwrap().unwrap(), vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn sse_非音频行一律当无音频() {
        // 心跳空行 / 注释 / event 头 / [DONE] / data 后空体：不得报错，否则整条流会被误杀
        for line in ["", ": ping", "event: message", "data:", "data: [DONE]"] {
            assert!(sse_line_to_pcm(line).is_none(), "行「{line}」应视为无音频");
        }
    }

    #[test]
    fn sse_无音频字段的chunk跳过() {
        // 对齐官方 Python 示例 if not chunk.choices: continue（纯文本 chunk / 末尾 usage）
        for chunk in [
            serde_json::json!({ "choices": [] }),
            serde_json::json!({ "choices": [{ "delta": { "content": "你好" } }] }),
            serde_json::json!({ "choices": [{ "delta": { "audio": null } }] }),
            serde_json::json!({ "usage": { "total_tokens": 1 } }),
        ] {
            assert!(sse_line_to_pcm(&format!("data: {chunk}")).is_none());
        }
    }

    #[test]
    fn sse_格式错误才报错() {
        assert!(matches!(sse_line_to_pcm("data: {not json"), Some(Err(_))));
        let bad_b64 = format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "audio": { "data": "!!!" } } }] })
        );
        let err = sse_line_to_pcm(&bad_b64).unwrap().unwrap_err();
        assert!(err.contains("base64"), "实际: {err}");
    }

    #[test]
    fn wav容器_识别并剥出data子块() {
        // 兼容模式若把音频整体塞进 WAV，宿主契约（pcm_s16le）会被 44 字节头污染
        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[12..16].copy_from_slice(b"fmt ");
        wav[16..20].copy_from_slice(&16u32.to_le_bytes());
        wav[36..40].copy_from_slice(b"data");
        wav[40..44].copy_from_slice(&4u32.to_le_bytes());
        let pcm = [1u8, 2, 3, 4];
        wav.extend_from_slice(&pcm);
        assert_eq!(strip_wav_container(&wav).unwrap(), pcm.to_vec());
    }

    #[test]
    fn wav容器_裸pcm原样返回不误剔() {
        // 24kHz 音频块开头恰好凑出 "RIFF" 的概率极低，但仍要求：非 WAV 一律不动
        assert!(strip_wav_container(&[0x00, 0x01, 0x02, 0x03]).is_none());
        let mut fake = vec![0u8; 20];
        fake[0..4].copy_from_slice(b"RIFF");
        assert!(strip_wav_container(&fake).is_none(), "太短不足以是 WAV");
    }

    #[test]
    fn wav容器_尺寸字段超出实际长度时按实际截断() {
        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[12..16].copy_from_slice(b"fmt ");
        wav[36..40].copy_from_slice(b"data");
        wav[40..44].copy_from_slice(&9999u32.to_le_bytes()); // 谎报长度
        wav.extend_from_slice(&[9u8; 6]);
        assert_eq!(strip_wav_container(&wav).unwrap().len(), 6);
    }

    #[test]
    fn 字节对齐_奇数尾巴进下一块() {
        // 宿主按 chunks_exact(2) 转采样，落单字节会被静默丢弃 → 一旦丢就永久相位错开
        let mut carry: Option<u8> = None;
        let a = align_bytes(&mut carry, &[0x34, 0x12, 0x78]);
        assert_eq!(a, vec![0x34u8, 0x12]);
        assert_eq!(carry, Some(0x78));
        let b = align_bytes(&mut carry, &[0x56]);
        assert_eq!(b, vec![0x78u8, 0x56]);
        assert_eq!(carry, None);
    }

    #[test]
    fn mime_五种官方格式() {
        assert_eq!(mime_for_ext("mp3").unwrap(), "audio/mpeg");
        assert_eq!(mime_for_ext("WAV").unwrap(), "audio/wav");
        assert_eq!(mime_for_ext("m4a").unwrap(), "audio/mp4");
        assert_eq!(mime_for_ext("flac").unwrap(), "audio/flac");
        assert_eq!(mime_for_ext("ogg").unwrap(), "audio/ogg");
        assert!(mime_for_ext("aac").is_err());
    }

    #[test]
    fn 音色解析_预置与克隆() {
        // 预置：用配置的基模型，voice 原样透传
        std::env::set_var("MIMO_TTS_MODEL", "mimo-v2.5-tts");
        let (model, voice) = resolve_voice(Some("冰糖")).unwrap();
        assert_eq!(model, "mimo-v2.5-tts");
        assert_eq!(voice.as_deref(), Some("冰糖"));
        // 缺省音色
        let (model, voice) = resolve_voice(None).unwrap();
        assert_eq!(model, "mimo-v2.5-tts");
        assert_eq!(voice.as_deref(), Some(DEFAULT_VOICE));
        // 克隆：切 voiceclone 模型，voice = 样本 data URI（此处无 VA_CLONE_SAMPLE → 中文错）
        std::env::remove_var("VA_CLONE_SAMPLE");
        let err = resolve_voice(Some("clone")).unwrap_err();
        assert!(err.contains("克隆"), "实际: {err}");
        std::env::remove_var("MIMO_TTS_MODEL");
    }

    #[test]
    fn 模型缺省值_空env回落() {
        std::env::remove_var("MIMO_TTS_MODEL");
        assert_eq!(configured_model(), DEFAULT_MODEL);
        std::env::set_var("MIMO_TTS_MODEL", "  ");
        assert_eq!(configured_model(), DEFAULT_MODEL);
        std::env::set_var("MIMO_TTS_MODEL", "mimo-v2.5-tts");
        assert_eq!(configured_model(), "mimo-v2.5-tts");
        std::env::remove_var("MIMO_TTS_MODEL");
    }
}
```

- [ ] **Step 4：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo test -p mimo-tts
```
预期：**编译失败**，报 `cannot find function build_request_body in this scope` 等未定义项。

- [ ] **Step 5：实现 `client.rs`**

把 `plugins/engines/mimo-tts/src/client.rs` 整体替换为（测试块保留在文件末尾）：

```rust
// MiMo TTS v2.5 协议层：请求体 / 响应抽取 / SSE 行解析 / WAV 容器剥离 / 克隆样本 URI。
//
// 全部为不碰网络的纯函数（克隆 URI 与模型解析只读环境变量），便于无网络单测。
// 协议事实（核实自 MiMo 官方文档，见 doc/MiMo插件化与流式默认设计.md §3.1）：
// - POST https://api.xiaomimimo.com/v1/chat/completions（OpenAI 兼容路径，但）
// - 鉴权头是 api-key（不是 Authorization: Bearer）；
// - 待合成文本放 role:"assistant" 的 content（不是 user）；
// - 非流式返回 choices[0].message.audio.data = base64 WAV；
// - 流式（stream:true + audio.format=pcm16）走 SSE，每行 data: 里
//   choices[0].delta.audio.data 是 base64 裸 PCM（24kHz/16bit/mono，无 WAV 头）。

use base64::Engine as _;
use std::path::Path;

/// MiMo TTS 端点
pub const ENDPOINT: &str = "https://api.xiaomimimo.com/v1/chat/completions";
/// 预置音色模型（可经 MIMO_TTS_MODEL 覆盖，供官方上新档位不改代码）
pub const DEFAULT_MODEL: &str = "mimo-v2.5-tts";
/// 克隆音色模型（voice == "clone" 时强制切换）
pub const CLONE_MODEL: &str = "mimo-v2.5-tts-voiceclone";
/// 未指定音色时的服务端默认音色
pub const DEFAULT_VOICE: &str = "mimo_default";
/// 官方口径：format=pcm16 即 24kHz PCM16LE 单声道裸 PCM
pub const STREAM_SAMPLE_RATE: u32 = 24_000;
/// 流式声道数
pub const STREAM_CHANNELS: u32 = 1;
/// 克隆样本上限（官方 10MB）。宿主只做通用防呆，精确上限在此
pub const CLONE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 非流式请求体
pub fn build_request_body(
    model: &str,
    text: &str,
    voice: Option<&str>,
    instruction: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "user", "content": instruction.unwrap_or("") },
            { "role": "assistant", "content": text }
        ],
        "audio": {
            "format": "wav",
            "voice": voice.unwrap_or(DEFAULT_VOICE)
        }
    })
}

/// 流式请求体：在非流式请求体上只改两处（复用以免两条路径参数漂移）
pub fn build_stream_request_body(
    model: &str,
    text: &str,
    voice: Option<&str>,
    instruction: Option<&str>,
) -> serde_json::Value {
    let mut body = build_request_body(model, text, voice, instruction);
    body["audio"]["format"] = serde_json::Value::String("pcm16".to_string());
    body["stream"] = serde_json::Value::Bool(true);
    body
}

/// 非流式响应 → wav 字节
pub fn extract_audio_data(response: &serde_json::Value) -> Result<Vec<u8>, String> {
    let data_str = response["choices"][0]["message"]["audio"]["data"]
        .as_str()
        .ok_or_else(|| "MiMo 响应格式异常：缺少 choices[0].message.audio.data".to_string())?;
    base64::engine::general_purpose::STANDARD
        .decode(data_str)
        .map_err(|e| format!("MiMo 响应音频 base64 解码失败: {e}"))
}

/// 一行 SSE → 裸 PCM 字节。
///
/// `None` = 本行无音频（空行 / `:` 注释 / `event:` 头 / `[DONE]` / 该 chunk 无音频字段）；
/// `Some(Err)` = 格式错误，调用方应中止本次流。
pub fn sse_line_to_pcm(line: &str) -> Option<Result<Vec<u8>, String>> {
    let payload = match line.strip_prefix("data:") {
        Some(p) => p.trim(),
        None => return None,
    };
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => return Some(Err(format!("流式 chunk JSON 解析失败: {e}"))),
    };
    // Value 索引对 Null / 越界安全：choices 缺失或空数组时拿到 Null，进而无 audio 字段
    let b64 = value["choices"][0]["delta"]["audio"]["data"].as_str()?;
    Some(
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("流式音频 base64 解码失败: {e}")),
    )
}

/// 若字节是完整 WAV（RIFF…WAVE），返回其 `data` 子块的裸 PCM；不是 WAV 返回 `None`。
///
/// 为什么要有这一步：官方称 voiceclone 的低延迟流式未上线时服务端会**自动退化为兼容模式**
/// （推理完成后一次性返回）。存档分支实测拿到过 1 块且能出声，但没断言过那 1 块是裸 PCM
/// 还是 WAV 容器。若是后者，宿主按 `pcm_s16le` 喂给 rodio → 开头 44 字节变爆音，
/// 且归档 WAV 会双头。这里做一次防御性剥离（真流式的块不会命中）。
pub fn strip_wav_container(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let cc = &bytes[off..off + 4];
        let size = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().ok()?) as usize;
        let body = off + 8;
        if cc == b"data" {
            let end = body.checked_add(size).map(|e| e.min(bytes.len())).unwrap_or(bytes.len());
            return Some(bytes[body..end].to_vec());
        }
        // WAV 子块按偶数字节对齐（奇数长度补一字节填充）
        off = body + size + (size & 1);
    }
    None
}

/// 把一块 PCM 裁到 2 字节对齐，落单字节暂存到 `carry` 供下一块拼接
pub fn align_bytes(carry: &mut Option<u8>, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if let Some(b) = carry.take() {
        out.push(b);
    }
    out.extend_from_slice(bytes);
    if out.len() % 2 == 1 {
        *carry = out.pop();
    }
    out
}

/// 按扩展名声明 MIME（MiMo 支持 mp3/wav/m4a/flac/ogg；MIME 与真实格式不符会被
/// 服务端判 "invalid audio format"，故这张表必须与官方一致）
pub fn mime_for_ext(ext: &str) -> Result<&'static str, String> {
    Ok(match ext.to_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        other => {
            return Err(format!(
                "不支持的克隆样本格式「{other}」，请改用 mp3 / wav / m4a / flac / ogg"
            ))
        }
    })
}

/// 读合成模型（空值回退默认）。env 名与 mimo-asr 的 MIMO_API_KEY 区分开，
/// 避免宿主必填 env 冲突检测拒载
pub fn configured_model() -> String {
    let raw = std::env::var("MIMO_TTS_MODEL").unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 读 API Key（由宿主通用配置机制按 manifest.config 注入）
pub fn read_api_key() -> Result<String, String> {
    std::env::var("MIMO_TTS_API_KEY")
        .map(|k| k.trim().to_string())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "未配置 MiMo API Key：请在「语音中心 → MiMo TTS」的插件设置中填写".to_string())
}

/// 把音色参数拆成 (模型, 请求里的 voice)
///
/// - `None` / 预置音色 id → 配置的基模型 + 原样透传
/// - `"clone"` → `mimo-v2.5-tts-voiceclone` + 样本 data URI
///   （样本路径来自宿主注入的通用环境变量 `VA_CLONE_SAMPLE`）
pub fn resolve_voice(voice: Option<&str>) -> Result<(String, Option<String>), String> {
    if voice == Some("clone") {
        return Ok((CLONE_MODEL.to_string(), Some(build_clone_voice_uri()?)));
    }
    Ok((configured_model(), Some(voice.unwrap_or(DEFAULT_VOICE).to_string())))
}

/// 克隆样本 → data URI（每次合成都要把样本传给服务端，故读一次算一次）
pub fn build_clone_voice_uri() -> Result<String, String> {
    let no_sample = "未导入克隆音色样本：请在「语音中心 → 音色克隆」导入一段 5–10 秒的清晰人声（mp3/wav/m4a/flac/ogg）";
    let raw = std::env::var("VA_CLONE_SAMPLE").map_err(|_| no_sample.to_string())?;
    if raw.trim().is_empty() {
        return Err(no_sample.to_string());
    }
    let path = Path::new(raw.trim());
    let bytes = std::fs::read(path)
        .map_err(|_| format!("克隆样本文件读取失败（{}）：文件可能已被删除，请重新导入", path.display()))?;
    let len = bytes.len() as u64;
    if len > CLONE_MAX_BYTES {
        return Err(format!(
            "克隆样本 {:.1}MB 超出 MiMo 上限 10MB，请换更短的样本",
            len as f64 / 1024.0 / 1024.0
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mime = mime_for_ext(ext)?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// 流式元信息 JSON（宿主据此装配播放器；当前契约只认 pcm_s16le）
pub fn stream_info_json() -> String {
    format!(
        r#"{{"format":"pcm_s16le","sample_rate":{},"channels":{}}}"#,
        STREAM_SAMPLE_RATE, STREAM_CHANNELS
    )
}
```

- [ ] **Step 6：跑测试确认通过**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
cargo test -p mimo-tts
```
预期：`test result: ok. 16 passed; 0 failed`（Step 3 的 16 个 `#[test]`）。

- [ ] **Step 7：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add plugins/engines/mimo-tts/Cargo.toml plugins/engines/mimo-tts/src/lib.rs plugins/engines/mimo-tts/src/client.rs plugins/engines/mimo-tts/src/stream.rs plugins/Cargo.lock
git commit -m "feat(插件): mimo-tts 协议层——请求体/SSE/WAV 容器剥离/克隆 URI 与单测

Co-Authored-By: Claude <noreply@anthropic.com>"
```

### 实施时的修正（已落地，后续 Task 按修正后的签名为准）

子代理审查发现两个本计划自写的问题，已修（commit 在 4f6fe95 之后）：

1. **环境变量读取从纯函数里剥出来**（Windows CRT 下并发测试交叉着 `set_var/remove_var`
   同一个变量不是线程安全的）。新签名：
   - `configured_model_from(raw: Option<String>) -> String`（纯），`configured_model()` 只做 env 薄壳
   - `resolve_voice_from(model: &str, voice: Option<&str>, sample_raw: Option<String>)`（纯），
     `resolve_voice(voice)` 为薄壳
   - `build_clone_voice_uri_from(sample_raw: Option<String>)`（纯）。原本还有一个
     `build_clone_voice_uri()` 薄壳，因 `resolve_voice` 已经把 `VA_CLONE_SAMPLE` 递下来，
     它成了无人调用的死代码 → **已删除**（零 warning 是门禁项）。
   **Task 2/4 调用时用薄壳版本**（`resolve_voice(voice)`），它们不依赖测试环境。
2. **测试不再把 `MIMO_TTS_MODEL` 设成与默认值同字面量串**（那是空断言，env 就算没读也能过）；
   改用哨兵 `"mimo-v9.9-tts-sentinel"`  断言。`configured_model()` / `resolve_voice()` 薄壳因此
   不进单测（它们的逻辑就一行 `env::var` ，由真机验收覆盖）。
3. 顺带三项质量修正：克隆样本**先 `metadata()` 判尺寸再 `fs::read`**（避免指到大文件时先全量吃内存）；
   读文件失败把底层 `std::io::Error` 一并带出（不再一律说成“文件可能已被删除”）；
   `voice` 为空/空串时回落 `DEFAULT_VOICE`（与 `configured_model` 的 trim 口径统一）。

---

## Task 2：插件流式会话 + 双导出（阻塞兼容面）

**Files:**
- Create: `plugins/engines/mimo-tts/src/stream.rs`
- Modify: `plugins/engines/mimo-tts/src/lib.rs`（替换 Task 1 的空壳）

线程模型照 `plugins/shared/minimax-tts-core/src/stream.rs`：`StreamItem{Chunk,Done,Fail}` +
`mpsc` + `next_chunk` 100ms 轮询 + `AtomicBool` 停止位 + `u64` 注册表。差异只在 worker：
MiniMax 是 WebSocket 且 `open()` 里同步握手；MiMo 是 HTTP SSE，故 **`open()` 同步发请求拿响应头**
（Key 错/网络不通在 start 阶段就返回，正好落在宿主 `STREAM_CONNECT_TIMEOUT=30s` 内），
body 交给 worker 逐行读。

- [ ] **Step 1：写失败测试（会话骨架 + info 契约 + 注册表）**

`plugins/engines/mimo-tts/src/stream.rs` 末尾先放测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 注册表_句柄自增且终态自动摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx);
        let id = reg.insert(session);
        assert_eq!(id, 1);
        let id2 = reg.insert(StreamSession::from_receiver(channel::<StreamItem>().1));
        assert_eq!(id2, 2);
        // Done → 拿到空 vec 且会话被摘除
        tx.send(StreamItem::Done).unwrap();
        assert_eq!(reg.next_chunk(id).unwrap(), Vec::new());
        assert_eq!(reg.next_chunk(id).unwrap_err(), "流式会话不存在或已停止");
    }

    #[test]
    fn 注册表_失败透出中文原因并摘除() {
        let reg = SessionRegistry::new();
        let (tx, rx) = channel::<StreamItem>();
        let id = reg.insert(StreamSession::from_receiver(rx));
        tx.send(StreamItem::Fail("MiMo 流式失败".into())).unwrap();
        assert_eq!(reg.next_chunk(id).unwrap_err(), "MiMo 流式失败");
        assert!(reg.next_chunk(id).is_err());
    }

    #[test]
    fn abort_置位后轮询亚秒返回错误() {
        let (_tx, rx) = channel::<StreamItem>();
        let session = StreamSession::from_receiver(rx);
        session.abort();
        let t = std::time::Instant::now();
        let err = session.next_chunk().unwrap_err();
        assert!(err.contains("中止"), "实际: {err}");
        assert!(t.elapsed() < Duration::from_millis(500), "abort 应亚秒生效，实际 {:?}", t.elapsed());
    }

    #[test]
    fn info契约_pcm_s16le_24k_单声道() {
        let info: serde_json::Value = serde_json::from_str(&crate::client::stream_info_json()).unwrap();
        assert_eq!(info["format"], "pcm_s16le");
        assert_eq!(info["sample_rate"], 24000);
        assert_eq!(info["channels"], 1);
    }
}
```

- [ ] **Step 2：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo test -p mimo-tts
```
预期：编译失败（`SessionRegistry` / `StreamSession` / `StreamItem` 未定义）。

- [ ] **Step 3：实现 `stream.rs`**

文件头（测试块之前）写入：

```rust
// MiMo 流式会话：HTTP SSE → 宿主拉模型（va_tts_stream_next）。
//
// ABI 是宿主主动拉块，而 MiMo 是「一次 POST、服务端连续推 SSE」，故：
// worker 线程独占响应体读取 → mpsc 队列 → next_chunk 轮询取块（100ms 周期）。
// abort 只置停止位：next_chunk 轮询里 ≤100ms 返回错误；worker 阻塞在 read_until
// 上不会立刻醒，但宿主此时已不再取块、连接随请求超时（5 分钟上限）自清，不悬死。
//
// 骨架与 minimax-tts-core/src/stream.rs 同构（会话字段/轮询常量/注册表照抄），
// 只把 WS 换成 blocking HTTP + BufReader::read_until(b'\n')。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::client::{align_bytes, sse_line_to_pcm, strip_wav_container, ENDPOINT};

/// 相邻音频块最大间隔：超过视为服务端失联（正常流式块间隔为亚秒级）
const CHUNK_GAP_TIMEOUT: Duration = Duration::from_secs(30);
/// next_chunk 轮询周期（决定 abort 生效时延上限）
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 整请求上限：MiMo 一次合成长文本远低于此值，仅作为 worker 卡死时的兜底
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

enum StreamItem {
    /// 一段 PCM（已在 worker 里做 2 字节对齐）
    Chunk(Vec<u8>),
    /// 流正常结束
    Done,
    /// 终态失败（worker 退出前最后一条）
    Fail(String),
}

/// 一次流式合成会话。全内部可变，可放进 Arc 供注册表跨线程持有。
pub struct StreamSession {
    /// Receiver 非 Sync，Mutex 包一层只为让注册表的 Sync 成立（宿主契约是单管线消费）
    rx: Mutex<Receiver<StreamItem>>,
    stop: Arc<AtomicBool>,
    /// 最近一次拿到数据块的时间（块间空闲超时基准）
    last_progress: Mutex<Instant>,
    /// worker 终态错误（channel 断开时兜出，避免丢真实原因）
    terminal: Mutex<Option<String>>,
}

impl StreamSession {
    /// 测试注入用：不起 worker，直接给一个接收端
    #[cfg(test)]
    fn from_receiver(rx: Receiver<StreamItem>) -> Self {
        Self {
            rx: Mutex::new(rx),
            stop: Arc::new(AtomicBool::new(false)),
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        }
    }

    /// 打开会话：**同步**发出 POST 并校验响应状态（Key 错/连不上在这里返回中文错），
    /// 成功再把响应体交给 worker 逐行读。
    pub fn open(api_key: String, body: serde_json::Value) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;
        let resp = client
            .post(ENDPOINT)
            .header("api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("连接 MiMo 失败：{e}（请检查网络与 API Key）"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(format!("MiMo 返回 {status}：{}", trim_for_msg(&text)));
        }

        let (tx, rx) = channel::<StreamItem>();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = std::thread::Builder::new()
            .name("mimo-tts-sse".into())
            .spawn(move || worker_run(stop, tx, resp))
            .map_err(|e| format!("启动流式读取线程失败: {e}"))?;
        // 句柄只用于给线程命名，不 join（worker 自收敛）
        std::mem::forget(handle);

        Ok(Self {
            rx: Mutex::new(rx),
            stop,
            last_progress: Mutex::new(Instant::now()),
            terminal: Mutex::new(None),
        })
    }

    /// 拉取下一块。Ok(非空)=一块 PCM；Ok(空)=流正常结束；Err=失败或已中止（中文原因）。
    pub fn next_chunk(&self) -> Result<Vec<u8>, String> {
        loop {
            let recv = self
                .rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(POLL_INTERVAL);
            match recv {
                Ok(StreamItem::Chunk(bytes)) => {
                    *self.last_progress.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    return Ok(bytes);
                }
                Ok(StreamItem::Done) => return Ok(Vec::new()),
                Ok(StreamItem::Fail(msg)) => {
                    *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                    return Err(msg);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.stop.load(Ordering::Relaxed) {
                        return Err("MiMo 流式合成已中止".into());
                    }
                    let idle = self.last_progress.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    if idle > CHUNK_GAP_TIMEOUT {
                        let msg = format!(
                            "MiMo 流式服务 {} 秒未返回音频，连接中断",
                            CHUNK_GAP_TIMEOUT.as_secs()
                        );
                        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                        return Err(msg);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let stored = self
                        .terminal
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    return Err(stored.unwrap_or_else(|| "流式会话已关闭".into()));
                }
            }
        }
    }

    /// 中止（幂等，任意线程可调）。next_chunk 轮询中 ≤100ms 感知返回。
    pub fn abort(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// worker：独占 SSE 响应体，按行解析、剥 WAV、2 字节对齐后推块
fn worker_run(stop: Arc<AtomicBool>, tx: Sender<StreamItem>, resp: reqwest::blocking::Response) {
    let mut reader = BufReader::new(resp.take());
    let mut line: Vec<u8> = Vec::new();
    let mut carry: Option<u8> = None;
    let mut got_audio = false;
    // 首块才做 WAV 探测：真流式的后续块是裸 PCM，不该被误当容器解析
    let mut first = true;

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = tx.send(StreamItem::Fail("MiMo 流式合成已中止".into()));
            return;
        }
        line.clear();
        let n = match reader.read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(e) => {
                let _ = tx.send(StreamItem::Fail(format!("MiMo 流式读取失败: {e}")));
                return;
            }
        };
        if n == 0 {
            break; // EOF：服务端收流
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if !feed_line_or_fail(&tx, text, &mut carry, &mut first, &mut got_audio) {
            return; // 已发出 Fail
        }
    }

    // 收尾：服务端最后一个 chunk 可能不带换行
    if !line.is_empty() {
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if !feed_line_or_fail(&tx, text, &mut carry, &mut first, &mut got_audio) {
            return;
        }
    }

    if !got_audio {
        let _ = tx.send(StreamItem::Fail(
            "MiMo 流式未返回音频（文本可能为空或全是符号）".into(),
        ));
        return;
    }
    let _ = tx.send(StreamItem::Done);
}

/// 处理一行 SSE。返回 false 表示已发送 Fail、worker 应退出。
fn feed_line_or_fail(
    tx: &Sender<StreamItem>,
    line: &str,
    carry: &mut Option<u8>,
    first: &mut bool,
    got_audio: &mut bool,
) -> bool {
    let bytes = match sse_line_to_pcm(line) {
        None => return true,
        Some(Err(e)) => {
            let _ = tx.send(StreamItem::Fail(e));
            return false;
        }
        Some(Ok(b)) => b,
    };
    let mut bytes = bytes;
    if *first {
        // 兼容模式可能整段返回 WAV：剥掉容器，保证交给宿主的仍是裸 PCM
        if let Some(pcm) = strip_wav_container(&bytes) {
            bytes = pcm;
        }
        *first = false;
    }
    let aligned = align_bytes(carry, &bytes);
    if aligned.is_empty() {
        return true;
    }
    *got_audio = true;
    tx.send(StreamItem::Chunk(aligned)).is_ok()
}

/// 错误消息进用户弹窗前必须剪短（HTTP body 可能是一大坨 HTML/JSON）
fn trim_for_msg(s: &str) -> String {
    const CAP: usize = 200;
    let flat: String = s.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
    if flat.chars().count() <= CAP {
        return flat;
    }
    let cut: String = flat.chars().take(CAP).collect();
    format!("{cut}…")
}

// ── 注册表 ────────────────────────────────────────────────

/// 会话注册表：句柄是 u64（跨 FFI 传 `VaStreamSession` 时用 `id as *mut c_void`）
pub struct SessionRegistry {
    map: Mutex<HashMap<u64, Arc<StreamSession>>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 注册新会话并返回句柄
    pub fn insert(&self, session: StreamSession) -> u64 {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::new(session));
        id
    }

    /// 拉一个块；会话到终态（结束/失败/中止）时自动移除
    pub fn next_chunk(&self, id: u64) -> Result<Vec<u8>, String> {
        let session = {
            let map = self.map.lock().unwrap_or_else(|e| e.into_inner());
            map.get(&id).cloned()
        };
        let Some(session) = session else {
            return Err("流式会话不存在或已停止".into());
        };
        let result = session.next_chunk();
        if matches!(&result, Ok(v) if v.is_empty()) || result.is_err() {
            self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
        result
    }

    /// 中止并释放会话（幂等）
    pub fn abort(&self, id: u64) {
        if let Some(session) = self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            session.abort();
        }
    }
}

/// 进程级单例会话表
pub fn global_sessions() -> &'static SessionRegistry {
    static REGISTRY: OnceLock<SessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SessionRegistry::new)
}
```

- [ ] **Step 4：跑测试确认通过**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
cargo test -p mimo-tts
```
预期：`test result: ok. 20 passed; 0 failed`（Task 1 的 16 + 本 Task 的 4）。

- [ ] **Step 5：写 `lib.rs` 双导出（含音色自述）**

`plugins/engines/mimo-tts/src/lib.rs` 整体替换：

```rust
// MiMo TTS 插件（小米·云端·流式）——由本体内置引擎下沉而来。
//
// 主路径是流式：宿主只要看到 va_tts_stream_* 三符号就直接走流式（没有开关、
// 也没有同引擎整段版回落）。同时导出阻塞合成，那是**老宿主兼容面**（不带流式
// ABI 的宿主加载它能当普通非流式插件用），本期新宿主不会调用它。
//
// 克隆音色：官方低延迟流式未上线时由服务端自动退化为兼容模式（一次性返回），
// 客户端不写降级分支——插件对预置与克隆发同一套请求，拿到几块就播几块。
// 「暂不支持低延迟」写在 voices.label 里由插件自述，宿主与前端零感知。
//
// 配置（宿主按 manifest.config 注入环境变量）：
//   MIMO_TTS_API_KEY   API Key（必填）
//   MIMO_TTS_MODEL     合成模型（留空默认 mimo-v2.5-tts）
//   VA_CLONE_SAMPLE    克隆样本绝对路径（宿主通用机制注入，非 MiMo 专属）

mod client;
mod stream;

pub use client::stream_info_json;

plugin_api::va_tts_plugin! {
    id: "mimo-tts",
    name: "MiMo TTS（小米·流式）",
    version: "0.1.0",
    audio_format: "wav",
    // 静态音色表：克隆项恒在——若按"样本文件是否存在"条件挂载，会出现
    // "看不到入口 → 永远导不进样本"的死锁；未导入就选克隆由插件返回中文错
    voices_json: r#"[{"id":"mimo_default","label":"默认 (mimo_default)"},{"id":"冰糖","label":"冰糖（女声）"},{"id":"茉莉","label":"茉莉（女声）"},{"id":"苏打","label":"苏打（男声）"},{"id":"白桦","label":"白桦（男声）"},{"id":"clone","label":"克隆：我的音色（暂不支持低延迟）"}]"#,
    synthesize: synthesize,
}

plugin_api::va_tts_stream_plugin! {
    start: stream_start,
    next: stream_next,
    stop: stream_stop,
}

/// 阻塞合成（老宿主兼容面）：一次成段 wav
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    let api_key = client::read_api_key()?;
    let (model, v) = client::resolve_voice(voice)?;
    let body = client::build_request_body(&model, text, v.as_deref(), None);
    let resp: serde_json::Value = reqwest::blocking::Client::new()
        .post(client::ENDPOINT)
        .header("api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("请求 MiMo 失败：{e}（请检查网络与 API Key）"))?
        .error_for_status()
        .map_err(|e| format!("MiMo 返回错误状态: {e}"))?
        .json()
        .map_err(|e| format!("MiMo 响应 JSON 解析失败: {e}"))?;
    client::extract_audio_data(&resp)
}

/// 打开流式会话：返回 (音频块格式自述 JSON, 会话句柄)
fn stream_start(text: &str, voice: Option<&str>) -> Result<(String, u64), String> {
    let api_key = client::read_api_key()?;
    let (model, v) = client::resolve_voice(voice)?;
    let body = client::build_stream_request_body(&model, text, v.as_deref(), None);
    let session = stream::StreamSession::open(api_key, body)?;
    let id = stream::global_sessions().insert(session);
    Ok((client::stream_info_json(), id))
}

/// 拉下一个 PCM 块（Ok 空 vec = 流正常结束）
fn stream_next(session: u64) -> Result<Vec<u8>, String> {
    stream::global_sessions().next_chunk(session)
}

/// 中止并释放会话（幂等）
fn stream_stop(session: u64) {
    stream::global_sessions().abort(session);
}

#[cfg(test)]
mod tests {
    #[test]
    fn 音色表_克隆项恒在且带提示() {
        // voices_json 由宏内联成静态串，这里重抄一份常量口径做断言
        let raw = r#"[{"id":"mimo_default","label":"默认 (mimo_default)"},{"id":"冰糖","label":"冰糖（女声）"},{"id":"茉莉","label":"茉莉（女声）"},{"id":"苏打","label":"苏打（男声）"},{"id":"白桦","label":"白桦（男声）"},{"id":"clone","label":"克隆：我的音色（暂不支持低延迟）"}]"#;
        let list: Vec<plugin_api::VoiceItem> = serde_json::from_str(raw).unwrap();
        assert_eq!(list.len(), 6);
        let clone = list.iter().find(|v| v.id == "clone").expect("克隆项必须恒在");
        assert!(clone.label.contains("暂不支持低延迟"), "实际: {}", clone.label);
    }
}
```

> `voices_json` 那份字面量在测试里重抄了一次，是为了让"忘了带提示"变成测试失败而不是
> 上线后的用户困惑。改音色表时两处一起改（宏里 + 测试里）。

- [ ] **Step 6：跑全部插件测试 + 编译产物检查**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo test -p mimo-tts
cargo build -p mimo-tts --release
```
预期：`test result: ok. 21 passed; 0 failed`；产出
`plugins\target\release\mimo_tts.dll`。

- [ ] **Step 7：宿主侧编译回归（未改宿主，应仍 170 passed）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib
```
预期：`170 passed; 0 failed; 4 ignored`。

- [ ] **Step 8：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add plugins/engines/mimo-tts/src/stream.rs plugins/engines/mimo-tts/src/lib.rs
git commit -m "feat(插件): mimo-tts 流式会话与双导出（SSE worker + 音色自述含低延迟提示）

Co-Authored-By: Claude <noreply@anthropic.com>"
```

### 实施时的修正（计划代码有四处编译不过，已就地修正）

1. **`reqwest::blocking::Response` 没有 `take()` 方法**（它本身就实现 `std::io::Read`）：
   `BufReader::new(resp.take())` → `BufReader::new(resp)`。原写法报
   `` `Response` is not an iterator ``（`take` 被当成 `Iterator::take` 去解）。
2. **`Arc<AtomicBool>` 不能既 move 进线程闭包又放进 `Self`**（`Arc` 不实现 `Copy`，E0382）：
   照 minimax-core 的写法补一行 `let worker_stop = Arc::clone(&stop);` 并用 `worker_stop` 进闭包。
3. **`assert_eq!(reg.next_chunk(id).unwrap(), Vec::new())` 类型推断歧义**（E0282/E0283）：
   改为 `let got = reg.next_chunk(id).unwrap(); assert!(got.is_empty(), …)`。
4. Task 1 遗留的 `build_clone_voice_uri()` 薄壳在本 Task 接上导出后仍无人调用
   （`resolve_voice` 已把 `VA_CLONE_SAMPLE` 递给 `_from`）→ 已删除，保证零 warning。

实测结果：`cargo test -p mimo-tts` = **21 passed / 0 failed / 0 warning**；
`cargo build -p mimo-tts --release` 产出 `plugins\target\release\mimo_tts.dll`（3548 KB）；
宿主 `cargo test --lib` 保持 **170 passed / 4 ignored**（本 Task 未动宿主）。

### 审查后的收口（子代理审查判 Needs changes，4 条 Important 已全部修完，现在 24 passed）

| 编号 | 问题 | 修法 |
|---|---|---|
| I-1 | 音色表测试只解析自己重抄的那份字面量，**从不去读宏内联的静态串** → 把宏里的 `clone` 项删掉测试依然全绿（恰好是规格 8 要防的那个死锁） | 新增断言 `std::str::from_utf8(crate::VA__VOICES).trim_end_matches('\0') == VOICES_EXPECT`（宏把字面量内联成 crate 根的 `static VA__VOICES`，带尾部 NUL） |
| I-2 | `*first = false` 在 `align_bytes` **之前**执行 → 网关先推一个空 base64 的 delta 时探测位被吃掉，真正的 WAV 首块逃过剥头 | 把关闭时机挪到“确实交出首个音频块之后”（`aligned.is_empty()` 早返回时保留 `first`） |
| I-3 | HTTP 200 + SSE 里推 `{"error":{"message":…}}`（OpenAI 兼容网关的标准中途失败形态）被当“本行无音频”跳过 → 半截音频被当正常结束落库且不报错；零块时还会误报成“文本可能为空” | `sse_line_to_pcm` 解析后先判 `error.message` → `Some(Err(…))`，只取 message 字段且过 `trim_for_msg`（不拼原始 payload，隐私红线）；新增 1 个单测 |
| I-4 | 空闲计时从会话创建起算，`CHUNK_GAP_TIMEOUT=30s` 同时充当**首块**预算 → 克隆（兼容模式全文推理才出块）长句会被误判失联 | 新增 `got_first: AtomicBool`；首块前用 `FIRST_CHUNK_TIMEOUT=120s`，首块后用 30s。**待 Task 4 探针实测长句首块耗时后回看此值** |

顺带采纳的 Minor：`strip_wav_container` 对 `size == 0`/谎小（流式写头未回填长度的常见做法）改为**取到末尾**而不是切空（+1 单测）；删掉 `worker_run` 里永不可达的 EOF 残行补处理块（`read_until` 在 EOF 会带残行返回 `Ok(n>0)`，已在循环内喂过）；阻塞 `synthesize` 补 `connect_timeout(15s)/timeout(300s)`（无超时会在宿主放弃等待后一直挂着，重试一次漏一条线程）；`trim_for_msg` 上提到 `client.rs` 作为 `pub(crate)` 共用，worker 侧读取失败文案也跟着剪短；删掉多余的 `pub use client::stream_info_json;`。

**未采纳（有意保留）**：把两条路径的 HTTP 客户端/鉴权头收成一个 `client::post_json()`——收口确实能消除 `api-key` 字面量重复，但会把“阻塞面”与“流式面”的超时差异揉进同一函数，本期先不动，已登记为下一批重构候选。`serde`/`hound` 依赖目前无源码引用（`hound` 是 Task 4 探针要用的），Task 4 落地后若仍无用再删。

修正后实测：`cargo test -p mimo-tts` = **24 passed / 0 failed / 0 warning**。

---

## 执行交接日志（每次停顿/切模型前必须更新本节）

| Task | 状态 | 提交 |
|---|---|---|
| 1 插件协议层 | 完成（含审查修正） | `4f6fe95` `ee89e05` |
| 2 流式会话 + 双导出 | 完成（含审查修正 I-1..I-4） | `3c2fcec` `96acedb` |
| 3 打包与内置分发 | 完成（两轮审查均 Approved；含 zip 可复现修正） | `a3df6d2` `c4dfb5f` `82337ec` + 后续收口 |
| 4–13 | 未开始 | — |

下一个动作：**Task 4 Step 1**（在 `plugins/engines/mimo-tts/src/stream.rs` 的 `mod tests` 末尾追加两个
`#[ignore]` 实网探针，再写 `plugins/scripts/test-mimo-stream.ps1`）。**这步要真实的 `MIMO_TTS_API_KEY`**，
拿不到 Key 就只能提交探针代码、不能宣称观测完成（探针要真跑，拿到几块 / 首块是不是 WAV 这两件事必须实装回答）。
另需回看：`FIRST_CHUNK_TIMEOUT=120s` 是否够长句首块（探针实测后定）；`hound` 依赖到本 Task 才真正被用到。

交接提醒（容易错的三件事）：
1. 每条 `cargo` 命令都必须先显式设 `CARGO_TARGET_DIR`（宿主用 `src-tauri\target`，插件用 `plugins\target`，两者不可互换）——曾有会话把全局 target 指到别的工作树，直接覆盖了别人的 exe。
2. 提交一律逐文件 `git add`，禁止 `git add -A`/`.`；`src-tauri/resources/plugins/*.zip` 是跟踪文件，新 zip 必须逐个 add（连删除也要 add）。
3. 每个 Task 的期望测试数写在各 Task 末尾，对不上就是漏删/漏测；当前基线：宿主 170 passed / 4 ignored（Task 6 后应降为 161），mimo-tts 24 passed / 0 warning。
4. `verify-index.ps1 -SkipDist` 的基线不是全绿：`minimax-tts-stream` 那条 `RES-CK` 是仓库既有问题（见 Task 3 实施记录）。
   判定本 Task 成败只看自己那一行是否 `OK`，不要为了让表格全绿去改别人的条目。
5. 任何重新构建插件 dll 的动作都会改变 zip sha（PE `TimeDateStamp`）；**打包与刷新索引 checksum 是同一个动作**，
   不能拆开提交（Task 3 的最终产物已在 `c4dfb5f` 一次锁死）。

---

## Task 3：打包脚本与内置分发（zip → resources → 插件索引）

**Files:**
- Create: `plugins/engines/mimo-tts/package.ps1`
- Create（脚本产物，入 git）: `src-tauri/resources/plugins/mimo-tts-0.1.0.zip`
- Modify: `plugins/plugins-index.json`（追加一条 `mimo-tts` 条目）

> 机制说明（照 `minimax-tts-stream` 的既有做法，零新发明）：
> `package.ps1` 产 zip 后同步进 `src-tauri/resources/plugins/`；启动时
> `manager.rs::bootstrap_bundled_zips` 扫 resources 顶层 zip、按 id 取最高版本自动安装；
> `registry.suppressed` 让"卸载"变成长期决定（不再自动重装）。
> **`.zip` 是 git 跟踪文件**，必须逐个 `git add`。
> `plugins/scripts/verify-index.ps1` 的 Part A 会**双向**校验：索引每条 `<id>-<version>.zip`
> 必须在 resources 下存在且 **sha256 等于 index.checksum**（注意是 **zip 的 sha，不是 dll 的 sha**，
> dll 的 sha 写在 manifest.checksum 里由 package.ps1 生成），resources 里多出来的 zip 也算失败。

- [ ] **Step 1：写 `plugins/engines/mimo-tts/package.ps1`**

模板 = `plugins/engines/minimax-tts-stream/package.ps1`（逐字对照，只换 id/名称/版本/env 名/描述）。
完整内容：

```powershell
# MiMo TTS（小米·流式）插件 - 打包脚本
#
# 用法（在 plugins/engines/mimo-tts 目录）：
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1            # 仅打包 zip
#   pwsh -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并安装到本地 exe 同级 plugins/
#
# -Install 前先关闭主程序（运行中的 dll 无法覆盖）。

param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

$PluginId   = "mimo-tts"
$PluginName = "MiMo TTS（小米·流式）"
$Version    = "0.1.0"
$MinAppVer  = "1.8.0"
$Desc       = "小米 MiMo 云端语音合成（流式版）：SSE 边合成边播，首响更快。需 API Key，预置音色 + 本地样本音色克隆（克隆由服务端退化为兼容模式，首响较慢）"

# 通用插件配置声明（宿主 ≥1.8.0 据此渲染设置面板并注入环境变量）。
# env 名必须独立于 mimo-asr 的 MIMO_API_KEY：两个必填 env 同名会触发宿主
# find_required_env_conflict 直接拒载（见 plugins/config.rs）。
# model 用 text 而非 select：manifest 校验强制 select 必须给出非空 options，
# 而官方档位会变；text 才能做到"上新档位改一个字符串、不发版"。
# help_url 是静态兜底，运行期会被远程配置 plugin_help_url 覆盖（换邀请码免发版）。
$ConfigDecl = @{
    help_url = "https://platform.xiaomimimo.com?ref=5P9J2B"
    fields   = @(
        @{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "从小米 MiMo 开放平台获取（与 MiMo ASR 用的是同一个 Key）"
            env         = "MIMO_TTS_API_KEY"
            required    = $true
        },
        @{
            key         = "model"
            type        = "text"
            label       = "合成模型"
            description = "留空即 mimo-v2.5-tts；官方上新档位可直接在此改名，无需更新软件"
            env         = "MIMO_TTS_MODEL"
            placeholder = "mimo-v2.5-tts"
            required    = $false
        }
    )
}

$PluginDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WsTarget  = (Resolve-Path (Join-Path $PluginDir "..\..")).Path   # Cargo workspace 根（plugins/），共享构建产物 target 所在
$DistDir   = Join-Path $PluginDir "dist"
$StageDir  = Join-Path $DistDir "package"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# -- 1. Build --
Write-Host "[1/4] Building plugin (release)..." -ForegroundColor Cyan
Push-Location $PluginDir
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$DllSrc = Join-Path $WsTarget "target\release\mimo_tts.dll"
if (-not (Test-Path $DllSrc)) { throw "Build artifact not found: $DllSrc" }

# -- 2. Stage directory --
Write-Host "[2/4] Generating manifest.json (with SHA-256)..." -ForegroundColor Cyan
Remove-Item -Recurse -Force $DistDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item $DllSrc (Join-Path $StageDir "plugin.dll")

$Hash = (Get-FileHash (Join-Path $StageDir "plugin.dll") -Algorithm SHA256).Hash.ToLower()
$Manifest = [ordered]@{
    id              = $PluginId
    name            = $PluginName
    version         = $Version
    type            = "tts_engine"
    platform        = @("windows")
    entry           = "plugin.dll"
    min_app_version = $MinAppVer
    checksum        = $Hash
    description     = $Desc
    category        = "remote"
    config          = $ConfigDecl
}
# 必须 -Depth 6：默认 Depth=2 会把 config.fields 的嵌套结构截成字符串
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 6),
    $Utf8NoBom
)

# -- 3. Compress zip --
Write-Host "[3/4] Packaging zip..." -ForegroundColor Cyan
$ZipPath = Join-Path $DistDir "$PluginId-$Version.zip"
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -Force
Write-Host "Package complete: $ZipPath" -ForegroundColor Green
Write-Host "  plugin.dll SHA-256: $Hash"

# Sync to app resource directory（内置分发：随安装包携带，可卸载）
$ResDir = Join-Path $PluginDir "..\..\..\src-tauri\resources\plugins"
New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
Remove-Item (Join-Path $ResDir "$PluginId-*.zip") -ErrorAction SilentlyContinue
Copy-Item $ZipPath $ResDir -Force
Write-Host "Synced to resources: $ResDir" -ForegroundColor Green
Write-Host ("  index.checksum must be zip SHA-256: {0}" -f (Get-FileHash $ZipPath -Algorithm SHA256).Hash.ToLower()) -ForegroundColor Yellow

# -- 4. Optional: Install locally --
if ($Install) {
    Write-Host "[4/4] Installing locally..." -ForegroundColor Cyan

    if (Get-Process -Name "TTSassist", "voiceassist" -ErrorAction SilentlyContinue) {
        throw "App is running. Close it first, then re-run with -Install"
    }

    $RepoRoot = (Resolve-Path (Join-Path $PluginDir "..\..\..")).Path
    $ExePaths = @(
        (Join-Path $RepoRoot "src-tauri\target\release\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\release\voiceassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\TTSassist.exe"),
        (Join-Path $RepoRoot "src-tauri\target\debug\voiceassist.exe")
    )
    $ExePath = $ExePaths | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $ExePath) {
        throw "No exe found (build first). Or set VA_PLUGINS_DIR env var."
    }
    if ($env:VA_PLUGINS_DIR) {
        $PluginsDir = $env:VA_PLUGINS_DIR
    } else {
        $PluginsDir = Join-Path (Split-Path -Parent $ExePath) "plugins"
    }
    $TargetDir = Join-Path $PluginsDir $PluginId
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

    Copy-Item (Join-Path $StageDir "plugin.dll")    $TargetDir -Force
    Copy-Item (Join-Path $StageDir "manifest.json") $TargetDir -Force

    $RegPath = Join-Path $PluginsDir "registry.json"
    if (Test-Path $RegPath) {
        $Reg = Get-Content $RegPath -Raw | ConvertFrom-Json
    } else {
        $Reg = [PSCustomObject]@{ plugins = @() }
    }
    $Entries = @()
    if ($Reg.plugins) {
        $Entries += @($Reg.plugins | Where-Object { $_.id -ne $PluginId })
    }
    $Entries += [PSCustomObject]@{
        id           = $PluginId
        version      = $Version
        installed_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:sszzz")
    }
    $Reg | Add-Member -NotePropertyName "plugins" -NotePropertyValue @($Entries) -Force
    [System.IO.File]::WriteAllText($RegPath, ($Reg | ConvertTo-Json -Depth 5), $Utf8NoBom)

    Write-Host "Installed to: $TargetDir" -ForegroundColor Green
    Write-Host "Launch the app and select '$PluginName' in engine settings."
}
```

- [ ] **Step 2：给脚本补 UTF-8 BOM（含中文的 .ps1 无 BOM 会被 PowerShell 5.1 按 GBK 解码报错）**

```powershell
$p = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\engines\mimo-tts\package.ps1"
$b = [System.IO.File]::ReadAllBytes($p)
if (-not ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)) {
    [System.IO.File]::WriteAllBytes($p, [byte[]](0xEF,0xBB,0xBF) + $b)
    "BOM 已补"
} else { "已有 BOM" }
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-bom.ps1
```
预期：`✓ BOM 检查通过：… 个 .json 无 BOM；… 个 .ps1…规则合规`。

- [ ] **Step 3：打包并同步进 resources**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\engines\mimo-tts\package.ps1
```
预期：末尾两行打印 `Package complete: …\mimo-tts-0.1.0.zip`、
`Synced to resources: …`，并有一行 **`index.checksum must be zip SHA-256: <64位小写hex>`** —— 记下这个值，Step 4 要用。

- [ ] **Step 4：`plugins/plugins-index.json` 追加条目**

在 `"plugins"` 数组内、`mimo-asr` 条目**之前**（保持按 id 字典序：edge-tts → genie-tts → hojo-tts → mimo-asr 的位置已知，mimo-tts 紧随 mimo-asr 之后、minimax-tts-stream 之前）插入，`<ZIP_SHA>` 换成 Step 3 打印的值：

```json
    {
      "id": "mimo-tts",
      "name": "MiMo TTS（小米·流式）",
      "version": "0.1.0",
      "download_url": "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/mimo-tts-0.1.0.zip",
      "mirror_url": "https://gitee.com/yihwan/TTSassist/raw/dist/mimo-tts-0.1.0.zip",
      "checksum": "<ZIP_SHA>",
      "description": "小米 MiMo 云端语音合成（流式版）：SSE 边合成边播、首响更快，需 API Key，预置音色 + 本地样本音色克隆（本体内置自动安装，可卸载）",
      "plugin_type": "tts_engine"
    },
```

> `mirror_url` 指向 Gitee `dist` 分支，由 `plugins/scripts/publish.ps1` 发版时自动上传；
> 本步只把条目登记进索引，实际上传在发版环节（不在本计划范围）。

- [ ] **Step 5：索引一致性校验**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\scripts\verify-index.ps1 -SkipDist
```
预期：表格全行 `OK`、`failures = 0`、`RESULT: ALL CONSISTENT`。
若报 `RES-CK` → checksum 填成了 dll 的 sha，改用 zip 的 sha 重来。

- [ ] **Step 6：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add plugins/engines/mimo-tts/package.ps1 plugins/plugins-index.json src-tauri/resources/plugins/mimo-tts-0.1.0.zip
git status --short   # 必须只有这三行（外加前两个 Task 已提交内容的干净状态）
git commit -m "feat(内置分发): mimo-tts 插件打包脚本 + 内置 zip + 索引条目

Co-Authored-By: Claude <noreply@anthropic.com>"
```

### Task 3 实施时的修正（审查后收口）

审查结论 Approved，无 Critical。但审查者实测“同一段代码不改、重跑 package.ps1，zip sha 会变”，
跟进后找到两个变异源，**都必须在本期修掉**（否则 Task 13 门禁与在线安装校验必然漂移）：

| 发现 | 现象 | 根因 | 处置 |
|---|---|---|---|
| **键序不稳定**（真因） | dll sha 不变、zip sha 每次不同 | `$ConfigDecl` 用普通 hashtable `@{}`，而 PS7(.NET Core) **每进程随机化字符串哈希**，`ConvertTo-Json` 出来的键序跨进程不同 → manifest 字节变 | `$ConfigDecl` 与其两个 field 子表全改 `[ordered]@{}`，产物变成声明序 |
| **条目 mtime** | 内容不变、仅 dll 被重新 touch 后重打包 → zip 变 | `Compress-Archive` 把条目 `LastWriteTime` 写进 zip | 压缩前把 `$StageDir` 内文件时间戳归一到固定 `$Epoch`（2020-01-01） |

修后实测：连跑两次 zip sha 相同（`f3508873…` 两次一致）；把 dll 的 mtime 拨回两天前再跑仍确定。
**仅剩的不可复现源是 dll 本身**——链接器往 PE 头写 `TimeDateStamp`，任何一次重链接都会改变 dll 字节。
这不是脚本缺陷，而是“不能假装二进制可复现”：所以 **每次重新构建插件都必须同步刷新索引 checksum**。

复审（针对 `c4dfb5f`）结论仍为 Approved，采纳一条 Minor：`Get-Date "2020-01-01 00:00:00Z"` 带 `Z` 会被当 UTC
再换算成本地时间（本机 UTC+8 → 写入 08:00），跨时区构建机会产出不同字节；改用
`$Epoch = [datetime]'2020-01-01T00:00:00'`（Kind=Unspecified，不做换算）。复审同时实测确认：
宿主解包不看条目时间戳（`install.rs` 按 `by_index` 顺序读、`manager.rs` 排序排的是路径、
版本取舍用 `version_less_than`），所以归一时间戳对安装零影响。

最终入库产物：dll sha `93468539a346d1c5c3faf1d1cd04ce7fdd8c32f4404c6743d3b0b186d21d5c36`、
zip sha `47d76f45f35afcc346a03d4f4cf2050661d114b6cd2c49388129feb8ede74e6d`（连跑两次相等、
且 zip 内 `manifest.checksum` == zip 内 dll 实际 sha，宿主内置安装的强校验能过）。

基线既有问题（**本次故意不动**，需用户决策）：

- `verify-index.ps1 -SkipDist` 在基线就有 1 条失败：`minimax-tts-stream` 报 `RES-CK`（索引 `0ad7d968…`，
  resources 里那份 zip 实际 `1d9e1ce7…`）。已核实在 `main` 的 `dc55a57` 就存在，本分支未碰过那个 zip。
  **不把索引改成 `1d9e1ce7`**：`publish.ps1` 发版时是以“新构建的 dist zip”重算 checksum 并上传该 zip
  作发布资产，所以索引里那个值很可能等于**线上真实资产**的 sha；改索引会把“CI 红灯”换成
  “用户点在线安装必校验失败”。真正该做的是重跑该插件打包并走一次发布（zip+索引+资产三者同步），超出本计划范围。
  它同时解释了上面那个键序缺陷如何制造漂移——模板脚本同样用 `@{}`。
- `tauri.conf.json` 的 `bundle.resources` 是 `"resources/plugins/*.zip"` 通配，新 zip **无需登记**。

有意未采纳（登记为下批重构候选）：

1. 脚本末尾自动跑 `verify-index.ps1` 并非 0 即 throw——会被上面那条基线红灯直接噎死，无法使用。
2. 脚本自己回写 `plugins-index.json`——`publish.ps1` 已经单写该文件，变双头写更容易不一致。
3. 索引 description 与 manifest description 不同（多了“本体内置自动安装，可卸载”）：`publish.ps1` 下次发布
   会以 manifest 为权威源覆盖索引——与现有 `minimax-tts-stream` 条目同口径，沿用惯例。

---

## Task 4：实网探针（预置多块 + 克隆首块容器观测）

**Files:**
- Modify: `plugins/engines/mimo-tts/src/stream.rs`（在文件末尾 `mod tests` 内追加 2 个 `#[ignore]` 探针）
- Create: `plugins/scripts/test-mimo-stream.ps1`

> 为什么现在就做（设计 §八 的两个顺序判断之二）："兼容模式返回的那一块到底是裸 PCM
> 还是 WAV 容器"是全设计唯一依赖实测的事实点，且只依赖插件的请求体、不依赖任何宿主改动。
> Task 1 已无条件实现 `strip_wav_container`（首块命中才剥），所以本探针的职责是
> **观测 + 断言"我们拿到的是可播 PCM"**，而不是决定要不要写剥头分支。

- [ ] **Step 1：在 `src/stream.rs` 的 `mod tests` 末尾追加两个探针测试**

```rust
    /// 把探针 PCM 封成 WAV 落到临时目录，供人耳试听（只手动跑，不进常规门禁）
    fn dump_probe_wav(pcm: &[u8], tag: &str) {
        let path = std::env::temp_dir().join(format!("mimo-probe-{tag}.wav"));
        let mut out = Vec::with_capacity(pcm.len() + 44);
        let rate = crate::client::STREAM_SAMPLE_RATE;
        let data_len = pcm.len() as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(pcm);
        std::fs::write(&path, &out).ok();
        println!("[probe] 试听文件: {}", path.display());
    }

    /// 实网探针①：预置音色应当是"多块"真流式。
    /// 跑法：plugins/scripts/test-mimo-stream.ps1（或手动 cargo test --lib -- --ignored --nocapture）
    #[test]
    #[ignore = "需真实 MIMO_TTS_API_KEY，手动运行"]
    fn 实网探针_预置音色应为多块流式() {
        let key = std::env::var("MIMO_TTS_API_KEY").expect("请先设置 MIMO_TTS_API_KEY");
        let text = "你好，这是一段流式合成的探针。今天天气不错，适合出门散步，我们边走边多聊几句。";
        let body = crate::client::build_stream_request_body(
            &crate::client::configured_model(),
            text,
            Some(crate::client::DEFAULT_VOICE),
            None,
        );
        let session = StreamSession::open(key.trim().to_string(), body).expect("打开流式会话失败");
        let t0 = Instant::now();
        let mut chunks = 0usize;
        let mut first_ms = 0u64;
        let mut pcm = Vec::<u8>::new();
        loop {
            match session.next_chunk() {
                Ok(b) if b.is_empty() => break,
                Ok(b) => {
                    if chunks == 0 {
                        first_ms = t0.elapsed().as_millis() as u64;
                    }
                    chunks += 1;
                    pcm.extend_from_slice(&b);
                }
                Err(e) => panic!("拉块失败: {e}"),
            }
        }
        let ms_total = t0.elapsed().as_millis();
        let audio_ms = pcm.len() as u64 * 1000 / (crate::client::STREAM_SAMPLE_RATE as u64 * 2);
        println!("[probe] 首块 {first_ms}ms / 块数 {chunks} / 音频 {audio_ms}ms / 总耗时 {ms_total}ms");
        assert!(!pcm.starts_with(b"RIFF"), "流式块不应带 WAV 容器头");
        assert_eq!(pcm.len() % 2, 0, "PCM 必须 2 字节对齐");
        assert!(chunks > 1, "预置音色未拿到多块：服务端可能退化为整段返回（持续如此需重估流式收益）");
        dump_probe_wav(&pcm, "preset");
    }

    /// 实网探针②：克隆音色（服务端兼容模式）首块必须被处理成可播 PCM。
    /// `strip_wav_container` 已在 worker 首块处生效，故这里断言"拿不到 RIFF 开头"。
    /// 需额外设置 VA_CLONE_SAMPLE 指向一段 5–10 秒人声样本。
    #[test]
    #[ignore = "需真实 MIMO_TTS_API_KEY + VA_CLONE_SAMPLE，手动运行"]
    fn 实网探针_克隆音色首块不得带容器头() {
        let key = std::env::var("MIMO_TTS_API_KEY").expect("请先设置 MIMO_TTS_API_KEY");
        let sample = std::env::var("VA_CLONE_SAMPLE").expect("请先设置 VA_CLONE_SAMPLE=样本绝对路径");
        assert!(
            std::path::Path::new(&sample).exists(),
            "VA_CLONE_SAMPLE 指向的文件不存在: {sample}"
        );
        let text = "这是一段克隆音色的流式探针，用来确认开头没有爆音。";
        let (model, voice) = crate::client::resolve_voice(Some("clone")).expect("克隆请求体构造失败");
        let body = crate::client::build_stream_request_body(&model, text, voice.as_deref(), None);
        let session = StreamSession::open(key.trim().to_string(), body).expect("打开流式会话失败");
        let mut chunks = 0usize;
        let mut first_head = [0u8; 4];
        let mut got_first = false;
        let mut pcm = Vec::<u8>::new();
        loop {
            match session.next_chunk() {
                Ok(b) if b.is_empty() => break,
                Ok(b) => {
                    if !got_first {
                        first_head.copy_from_slice(&b[0..4]);
                        got_first = true;
                    }
                    chunks += 1;
                    pcm.extend_from_slice(&b);
                }
                Err(e) => panic!("克隆流式拉块失败: {e}"),
            }
        }
        println!("[probe] 克隆：块数 {chunks} / 首块前 4 字节 {:?}", first_head);
        assert!(chunks >= 1, "克隆音色未返回任何音频块");
        assert_ne!(&first_head, b"RIFF", "首块仍是 WAV 容器头：strip_wav_container 未生效");
        assert_eq!(pcm.len() % 2, 0, "PCM 必须 2 字节对齐");
        dump_probe_wav(&pcm, "clone");
    }
```

- [ ] **Step 2：跑一次编译（含测试）确认探针可编**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
cd engines\mimo-tts
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo check --tests
```
预期：`Finished` 且无 error（探针不参与 `cargo test` 默认运行，`--ignored` 才跑）。

- [ ] **Step 3：写冒烟脚本 `plugins/scripts/test-mimo-stream.ps1`**

模板同 `test-minimax-stream.ps1`，但**不重复实现协议**：脚本负责"取本机 Key/样本 → 设 env → 跑 crate 内的 `#[ignore]` 探针"，
这样探针验证的是**插件真实代码路径**（Task 2 的 worker/对齐/剥头逻辑），而不是另一份独立实现。

```powershell
# MiMo TTS（流式）实网冒烟：跑 mimo-tts crate 内的 #[ignore] 探针
#
# 用法：
#   pwsh -ExecutionPolicy Bypass -File .\test-mimo-stream.ps1           # 只跑预置音色探针
#   pwsh -ExecutionPolicy Bypass -File .\test-mimo-stream.ps1 -Clone    # 连克隆探针一起跑
#
# API Key 与克隆样本默认从本机 settings.json 读取（免手工填 env）；
# 也可先用 $env:MIMO_TTS_API_KEY / $env:VA_CLONE_SAMPLE 显式指定。
# 注意：本脚本不打印 Key（隐私规则）。

param(
    [switch]$Clone
)

$ErrorActionPreference = "Stop"

$ScriptsDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = (Resolve-Path (Join-Path $ScriptsDir "..\..")).Path
$PluginDir  = Join-Path $RepoRoot "plugins\engines\mimo-tts"
$TargetDir  = Join-Path $RepoRoot "plugins\target"

# -- 1. Key：显式 env 优先，否则读本机 settings.json（mimo_api_key 或 plugin_config 里的新位置）--
if (-not $env:MIMO_TTS_API_KEY) {
    $settingsPath = Join-Path $env:APPDATA "com.voiceassist.app\settings.json"
    if (Test-Path $settingsPath) {
        $raw = (Get-Content $settingsPath -Raw).TrimStart([char]0xFEFF)
        $obj = $raw | ConvertFrom-Json
        $k = $obj.plugin_config.'mimo-tts'.api_key
        if (-not $k) { $k = $obj.mimo_api_key }
        if ($k) { $env:MIMO_TTS_API_KEY = $k }
    }
}
if (-not $env:MIMO_TTS_API_KEY) {
    Write-Host "[SKIP] 拿不到 MIMO_TTS_API_KEY：本机 settings.json 里没有，也没设环境变量" -ForegroundColor Yellow
    exit 1
}
Write-Host "[OK] API Key 就绪（长度 $($env:MIMO_TTS_API_KEY.Length)）" -ForegroundColor Green

# -- 2. 克隆样本：仅在 -Clone 时需要 --
if ($Clone -and -not $env:VA_CLONE_SAMPLE) {
    $settingsPath = Join-Path $env:APPDATA "com.voiceassist.app\settings.json"
    if (Test-Path $settingsPath) {
        $raw = (Get-Content $settingsPath -Raw).TrimStart([char]0xFEFF)
        $rel = ($raw | ConvertFrom-Json).clone_voice_path
        if ($rel) {
            $abs = Join-Path (Join-Path $env:APPDATA "com.voiceassist.app") $rel
            if (Test-Path $abs) { $env:VA_CLONE_SAMPLE = $abs }
        }
    }
}
if ($Clone -and -not $env:VA_CLONE_SAMPLE) {
    Write-Host "[SKIP] -Clone 需要克隆样本，但本机没导入过（或文件已删）" -ForegroundColor Yellow
    exit 1
}

# -- 3. 跑探针 --
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = $TargetDir
Push-Location $PluginDir
try {
    Write-Host "[RUN] mimo-tts #[ignore] 实网探针" -ForegroundColor Cyan
    cargo test --lib -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) { throw "探针未通过（见上方输出）。首块若报 RIFF 未剥离，检查 stream.rs worker 首块处理" }
} finally {
    Pop-Location
}
Write-Host "[DONE] 探针通过；试听 wav 在 `%TEMP%\\mimo-probe-*.wav`" -ForegroundColor Green
```

- [ ] **Step 4：真跑（用户机器上 Key 与样本通常已在 settings.json）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\scripts\test-mimo-stream.ps1 -Clone
```
预期两行 `[probe]` 输出，形如：
- `[probe] 首块 700~1200ms / 块数 25~26 / 音频 …ms`（预置：多块 = 真流式）
- `[probe] 克隆：块数 1 / 首块前 4 字节 [x, y, z, w]`（**不得是 `[82, 73, 70, 70]` = "RIFF"**）

把两条实测数字（首块耗时、块数、克隆首块头字节）原样记进 Task 13 的文档回写，
作为"要不要补预缓冲/水位"的唯一证据。**若脚本因拿不到 Key 而 `[SKIP]` 退出：
如实报告并停下问用户要 Key，不得跳过本步继续往下做。**

- [ ] **Step 5：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add plugins/engines/mimo-tts/src/stream.rs plugins/scripts/test-mimo-stream.ps1
git commit -m "test(插件): mimo-tts 实网探针（预置多块 + 克隆首块容器断言）与冒烟脚本

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5：mimo-asr 补配置声明 + 宿主换注入（**硬性同提交**）

**Files:**
- Modify: `plugins/engines/mimo-asr/package.ps1`（版本 0.1.0→0.1.1 + `config` 声明 + `-Depth 6`）
- Create（产物）: `src-tauri/resources/plugins/mimo-asr-0.1.1.zip`（同版本旧 zip 被脚本删除 → 需 `git add` 记录删除）
- Modify: `plugins/plugins-index.json`（mimo-asr 条目 version/checksum/description）
- Modify: `src-tauri/src/lib.rs:128-131`（删 `MIMO_API_KEY` 顶层注入，改为 `VA_CLONE_SAMPLE` 启动注入）
- Modify: `src-tauri/src/commands/settings.rs:45-51`（删 `mimo_api_key` 运行期 env 同步块）

> 设计 §七 的硬约束：**mimo-asr 未重打包前就删宿主注入 = ASR 直接失 Key**。
> 因此本 Task 的 5 个文件必须落在**同一个 commit**，不允许拆成两次提交。

- [ ] **Step 1：改 `plugins/engines/mimo-asr/package.ps1`**

三处改动（其余保持原样）：

① 版本号一行改为：

```powershell
$Version    = "0.1.1"
```

② 在 `$Desc = …` 之后、`$PluginDir = …` 之前插入配置声明块：

```powershell
# 通用插件配置声明（宿主 ≥1.8.0 按 manifest 渲染设置面板并注入环境变量）。
# 背景：v0.1.0 的 MIMO_API_KEY 靠宿主 lib.rs 从顶层 mimo_api_key 直接 set_var，
# manifest 里没有任何 config 声明；宿主删掉那条注入后必须改走本机制，否则 ASR 失 Key。
# min_app_version 不必提高：老宿主仍走顶层注入，新宿主走 config 注入，两条路都通。
$ConfigDecl = @{
    help_url = "https://platform.xiaomimimo.com?ref=5P9J2B"
    fields   = @(
        @{
            key         = "api_key"
            type        = "secret"
            label       = "API Key"
            description = "从小米 MiMo 开放平台获取（与 MiMo TTS 用的是同一个 Key）"
            env         = "MIMO_API_KEY"
            required    = $true
        }
    )
}
```

③ `$Manifest` 里加一行 `config`，并把写文件的 `ConvertTo-Json` 改成 `-Depth 6`
（默认 Depth=2 会把 `fields` 数组里的对象截成 `@{…}` 字符串，宿主解析必然失败）：

```powershell
    description     = $Desc
    category        = "remote"
    timeout_secs    = 120
    config          = $ConfigDecl
}
# 注意：必须用无 BOM 写入；Depth 必须 ≥6 否则嵌套结构会被截断
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir "manifest.json"),
    ($Manifest | ConvertTo-Json -Depth 6),
    $Utf8NoBom
)
```

- [ ] **Step 2：重打包 mimo-asr**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\engines\mimo-asr\package.ps1
```
预期：产出 `mimo-asr-0.1.1.zip` 并同步进 resources（脚本会先删同 id 旧 zip，
所以 `src-tauri/resources/plugins/mimo-asr-0.1.0.zip` **会消失**——这是预期的，Step 6 用 `git add` 记录删除）。
记下打印的 zip SHA-256（脚本若未打印，用 Step 3 的命令现算）。

- [ ] **Step 3：改索引里 mimo-asr 条目**

```powershell
(Get-FileHash "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\resources\plugins\mimo-asr-0.1.1.zip" -Algorithm SHA256).Hash.ToLower()
```
把 `plugins/plugins-index.json` 里 `mimo-asr` 条目改为（`<ZIP_SHA>` 换成上面的值）：

```json
    {
      "id": "mimo-asr",
      "name": "MiMo ASR（小米·云端）",
      "version": "0.1.1",
      "download_url": "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/mimo-asr-0.1.1.zip",
      "mirror_url": "https://gitee.com/yihwan/TTSassist/raw/dist/mimo-asr-0.1.1.zip",
      "checksum": "<ZIP_SHA>",
      "description": "小米 MiMo-V2.5-ASR 云端语音识别，支持中英文及自动语种检测（v0.1.1 起 API Key 改走通用插件配置声明）",
      "plugin_type": "asr_engine"
    },
```

- [ ] **Step 4：宿主 `lib.rs` 换注入**

把 `src-tauri/src/lib.rs` L128-131 整块替换（**删 `MIMO_API_KEY`，加 `VA_CLONE_SAMPLE`**）：

```rust
            // 克隆样本通用环境变量：宿主只把"已导入样本的绝对路径"告诉插件，
            // 格式/大小/用法全部由插件自决（基座不认识任何引擎，见 AGENTS.md 基座合同）。
            // API Key 不再在此硬编码注入：改由各插件 manifest 的 config 声明，
            // 统一走下方 plugin_manager.inject_config_env（旧 MIMO_API_KEY 顶层注入已删）。
            {
                let sample = if settings.clone_voice_path.is_empty() {
                    String::new()
                } else {
                    data_dir
                        .join(&settings.clone_voice_path)
                        .to_string_lossy()
                        .into_owned()
                };
                if sample.is_empty() || !std::path::Path::new(&sample).exists() {
                    std::env::remove_var("VA_CLONE_SAMPLE");
                } else {
                    std::env::set_var("VA_CLONE_SAMPLE", &sample);
                }
            }
```

同时把 L112 的注释改成不含引擎字面量的说法（基座护栏要扫注释）：

```rust
            // 兼容迁移：老设置里的内置引擎标识 → 对应插件 id（幂等，直接改文件）
            plugins::migrate_legacy_engine(&data_dir);
```

- [ ] **Step 5：宿主 `commands/settings.rs` 删运行期 Key 同步块**

删掉 L44-51 这段（`update_setting` 里对 `mimo_api_key` 的特殊处理），并在原位置留一行说明：

```rust
    // 遗留顶层键不再同步环境变量：API Key 统一经 manifest config 注入
    //（见 plugins::config::inject_fields 与 set_plugin_config 命令）
```

> 保留 `storage::settings::update_setting` 白名单里的 `"mimo_api_key" =>` 臂
> （字段本版本不删，前端类型与测试仍在用），只删上面这段 env 副作用。

- [ ] **Step 6：宿主编译与测试**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib
pwsh -NoProfile -ExecutionPolicy Bypass -File ..\plugins\scripts\verify-index.ps1 -SkipDist
```
预期：`170 passed; 0 failed; 4 ignored`（本 Task 未增删宿主测试）+ `ALL CONSISTENT`。

- [ ] **Step 7：一次性提交全部 5 处（不可拆）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add plugins/engines/mimo-asr/package.ps1 plugins/plugins-index.json src-tauri/resources/plugins/mimo-asr-0.1.1.zip src-tauri/resources/plugins/mimo-asr-0.1.0.zip src-tauri/src/lib.rs src-tauri/src/commands/settings.rs
git status --short
git commit -m "feat(插件): mimo-asr 声明 config 并重打包 + 宿主删 MIMO_API_KEY 硬编码注入、改注入 VA_CLONE_SAMPLE

两者必须同提交：删注入后未带 config 声明的旧 zip 会让 ASR 直接拿不到 Key。

Co-Authored-By: Claude <noreply@anthropic.com>"
```

## Task 6：宿主删内置 MiMo（引擎分发收口 + 默认引擎改 edge-tts）

**Files:**
- Delete: `src-tauri/src/tts/mimo.rs`（221 行，8 个单测 + 1 个 `#[ignore]` 实网测试 → 已由 Task 1/2 的插件同构单测接替）
- Modify: `src-tauri/src/tts/mod.rs`（删 `pub mod mimo;`、删 `build_engine` 及其 2 个测试）
- Modify: `src-tauri/src/commands/tts.rs`（删 `build_clone_voice_uri` / `generate_mimo` / `"" | "mimo"` 臂 / `use …MimoEngine`）
- Modify: `src-tauri/src/tts/traits.rs:46`（`NoApiKey` 文案去 MiMo 特指）
- Modify: `src-tauri/src/commands/diag.rs:650`（样例串 `engine=mimo`）
- Modify: `src-tauri/src/commands/tts_stream.rs:120`（注释里的引擎字面量）
- Modify: `src-tauri/src/storage/types.rs:203,377`（默认引擎 + 同步测试）
- Modify: `src-tauri/src/storage/settings.rs:33,255`（空串归一 + 同步测试 + 新增归一测试）

> **顺序说明**：本 Task 把内置 MiMo 删干净，而 `tts_engine="mimo"` 的老设置要到 Task 7
> 才迁移。两次提交之间，老标识会落到「按插件 id 查表」分支并报
> `引擎「mimo」不可用：插件未安装或加载失败…`。**这是中间态、不发布**，Task 7 紧随修复。
> 之所以先删后迁：删本体代码是纯减法，先做能立刻看清还剩哪些引用点（编译器帮忙找），
> 反过来先做迁移会让「内置仍可用」掩盖漏删的引用。

- [ ] **Step 1：写失败测试——空串引擎应归一到默认值**

在 `src-tauri/src/storage/settings.rs` 的 `mod tests` 内、`部分字段缺失用默认补齐` 之后插入：

```rust
    #[test]
    fn 空串引擎归一到默认值() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        // 老版本可能把 tts_engine 写成空串（而非缺字段）：不得落成空引擎，
        // 否则它会命中插件查表分支并报「引擎「」不可用」
        std::fs::write(d.join("settings.json"), r#"{"tts_engine":""}"#).unwrap();
        let s = load_settings(&d);
        assert_eq!(s.tts_engine, Settings::default().tts_engine, "空串应归一到默认引擎");
        assert!(!s.tts_engine.is_empty());
    }
```

- [ ] **Step 2：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib 空串引擎归一到默认值
```
预期：FAIL，`assertion failed`（当前实现只处理了字段缺失，空串被原样读进来）。

- [ ] **Step 3：默认值与归一落地**

① `src-tauri/src/storage/types.rs` L203：

```rust
            tts_engine: "edge-tts".to_string(),
```

② 同文件 L377 的测试断言同步：

```rust
        assert_eq!(s.tts_engine, "edge-tts");
```

③ `src-tauri/src/storage/settings.rs` L33（缺失字段与空串一律落默认）：

```rust
            tts_engine: v.get("tts_engine").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from).unwrap_or(default.tts_engine),
```

④ 同文件 L255 的测试断言同步：

```rust
        assert_eq!(s.tts_engine, "edge-tts", "缺失字段用默认");
```

- [ ] **Step 4：删 `src-tauri/src/tts/mimo.rs`**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
Remove-Item .\src-tauri\src\tts\mimo.rs
```

> 该文件里的 8 个单测（请求体 / 响应取音频 / 空 Key / name / category）与 1 个
> `#[ignore]` 实网测试，在 Task 1（`client.rs` 16 例）与 Task 2（`stream.rs` 4 例）里
> 已有覆盖面更全的同构版本，不存在丢失断言。

- [ ] **Step 5：改写 `src-tauri/src/tts/mod.rs`（整文件替换）**

`build_engine` 的唯一调用方就是内置 MiMo（插件引擎走 `PluginManager::build_engine`），
删掉 mimo 分支后它只剩「一律返回未知引擎」，是死代码——连函数带 2 个测试一起删：

```rust
// TTS 引擎模块入口。
// 内置引擎只保留 Moss（唯一尚未插件化的云端引擎）；edge、MiMo 等均已抽出为插件，
// 由 plugins::PluginManager 加载。内置与插件引擎的分发都在 commands/tts.rs，
// 这里不再有统一的引擎工厂函数（基座不认引擎身份，见 AGENTS.md 基座合同）。

pub mod traits;
pub mod moss;
```

- [ ] **Step 6：`src-tauri/src/commands/tts.rs` 删三处 + 收口分发**

① L1-11 的文件头与 import（删 `use crate::tts::mimo::MimoEngine;`，注释去引擎字面量）：

```rust
// generate_tts 命令：前端输入文本 → TTS 生成 → 写 messages.json → 广播。
// 支持多引擎分发：settings.tts_engine = 内置引擎（moss）| 插件 id（如 "edge-tts"）

use tauri::{AppHandle, Manager};
use crate::commands::AppState;
use crate::plugins::PluginManager;
use crate::storage::types::{Message, gen_id, now_iso};
use crate::tts::moss::MossEngine;
use crate::tts::traits::{TTSEngine, TTSParams};
use crate::sync::{notify_changed, EVENT_MESSAGE_CHANGED};
```

> 相对原文件的 import 差异（三行）：删 `use std::path::Path;`（只有 `build_clone_voice_uri`
> 在用）、删 `use crate::tts::mimo::MimoEngine;`、把 `use crate::tts::traits::{TTSEngine,
> TTSParams, TtsError};` 里的 `TtsError` 去掉（只有 `generate_mimo` 的返回类型在用）。
> **`TTSEngine` 必须保留**：插件引擎 `PluginEngine::generate` 是 trait 方法，不进作用域就调不动。
> `MossEngine::generate` 是 inherent 方法，不依赖 trait。以编译器的 unused 警告为最终依据，
> 零警告是 Task 13 的门禁项。

② 删 L14-41 整段 `build_clone_voice_uri`（含其上方的 MiMo 说明注释）。

③ 删 L43-87 整段 `generate_mimo`。

④ L120-179 的「读设置 + 分发」两段整体替换（`mimo_key` / `tts_model` / `clone_path` 不再读）：

```rust
    let (tts_engine, moss_key, moss_voice, plugin_voices, mic_send_enabled, mic_device, mic_volume) = {
        let s = state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        (
            s.tts_engine.clone(),
            s.moss_api_key.clone(),
            s.moss_voice_id.clone(),
            s.plugin_voices.clone(),
            s.mic_send_enabled,
            s.mic_output_device.clone(),
            s.mic_playback_volume,
        )
    };

    // 2. 据引擎分发：内置只剩 moss，其余引擎名一律按插件 id 查表
    let audio_path = match tts_engine.as_str() {
        "moss" => {
            if moss_key.is_empty() {
                return Err("请在设置中填写 Moss-TTS API Key".into());
            }
            if moss_voice.is_empty() {
                return Err("请在设置中填写 Moss-TTS voice_id".into());
            }
            let engine = MossEngine::new(moss_key, moss_voice, data_dir.clone());
            engine.generate(&text).await.map_err(|e| format!("{e}"))?
        }
        plugin_id => {
            // 其余引擎名按插件 id 查找（如 "edge-tts"）
            let engine = plugins
                .build_engine(plugin_id, &data_dir)
                .ok_or_else(|| {
                    format!("引擎「{plugin_id}」不可用：插件未安装或加载失败，请在插件管理中检查")
                })?;
            let voice = plugin_voices
                .get(plugin_id)
                .filter(|v| !v.is_empty())
                .map(|s| s.as_str());
            let params = TTSParams {
                text: &text,
                voice,
                instruction: None,
            };
            engine.generate(params).await.map_err(|e| format!("{e}"))?
        }
    };
```

> 删除的 `"" | "mimo"` 臂不再需要空串兜底：Step 3③ 已在 `load_settings` 归一。
> `tts_model` 字段本身**保留**（Task 7 才把它搬进 `plugin_voices`），只是这里不再读它。

- [ ] **Step 7：文案与注释去引擎特指**

① `src-tauri/src/tts/traits.rs` L45-47（文案去 MiMo 特指）：

```rust
    /// settings 中未配置 API Key
    #[error("未配置 API Key，请在设置或对应插件的配置中填写")]
    NoApiKey,
```

同文件 L83 的示例注释也跟着改（护栏连注释一起扫）：

```rust
    /// 引擎名称（内置引擎名或插件 id）
    fn name(&self) -> &str;
```

> `NoApiKey` 不会因删 MiMo 而变成未被构造的变体：`tts/moss.rs:59` 还在用它，
> 故无 dead-code 警告风险。

② `src-tauri/src/commands/diag.rs` L650（日志脱敏测试里自造的样本行）：

```rust
        log_info!("合成完成 text=\"今天股票会涨吗救救我\" engine=demo-tts");
```

> 用一个不存在的引擎名 `demo-tts`：这行只是“模拟一条含用户文本的日志”的载体，
> 不需要真实引擎；换成任何已知引擎只会给 Task 12 的基线多加一条无意义登记。

③ `src-tauri/src/commands/tts_stream.rs` L120 注释：

```rust
        // 未导出流式符号的引擎（内置引擎与老插件）：整体回落到既有阻塞管线（宿主不播，前端照旧整段播放）
```

- [ ] **Step 8：编译与测试**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo check --lib --tests
cargo test --lib
```
预期：`cargo check` **零 warning**（若报 unused import 回到 Step 6① 删干净）；
`cargo test --lib` = **161 passed; 0 failed; 3 ignored**。
算式：基线 170 − 8（mimo.rs）− 2（tts/mod.rs 两测）+ 1（Step 1 新增）= 161；
ignored 4 − 1（mimo.rs 的实网测试）= 3。**数字对不上说明删漏或删多了，回头核对。**

- [ ] **Step 9：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src-tauri/src/tts/mimo.rs src-tauri/src/tts/mod.rs src-tauri/src/tts/traits.rs src-tauri/src/commands/tts.rs src-tauri/src/commands/diag.rs src-tauri/src/commands/tts_stream.rs src-tauri/src/storage/types.rs src-tauri/src/storage/settings.rs
git status --short
git commit -m "feat(基座): 删除内置 MiMo 引擎，引擎分发只剩 moss + 插件查表，默认引擎改 edge-tts

- tts/mimo.rs 整体删除（单测已在插件 crate mimo-tts 内同构接替）
- tts/mod.rs 的 build_engine 是内置引擎专用工厂，随 MiMo 一起删除
- load_settings 对 tts_engine 空串归一到默认（原先只处理字段缺失）
- 老设置里的 tts_engine=\"mimo\" 暂时会报「引擎不可用」，下一个提交修复

Co-Authored-By: Claude <noreply@anthropic.com>"
```

> `git add` 一个被删除的路径是合法的（把删除记入索引）。
> 若 PowerShell 把 `\"` 转义搞坏 commit message，改用 `git commit -F <临时文件>`，
> 临时文件写完记得删——**不要**把临时文件 add 进仓库。

---

## Task 7：一次性迁移三段（引擎标识 / tts_model / 遗留 Key）

**Files:**
- Modify: `src-tauri/src/plugins/mod.rs:29-70`（`migrate_legacy_engine` 改多步）+ `mod.rs` 测试区
- Modify: `src-tauri/src/storage/settings.rs:107-150`（`LEGACY_PLUGIN_KEY_MAP` 加两行 + 快照修复 + 两处 match 臂）+ 测试区

> 设计 §3.4。三段迁移都幂等、每次启动执行；
> `migrate_legacy_engine` 直接改 `settings.json` 文件（不经 `Settings` 结构体，
> 因为 `edge_voice` 等旧字段结构体里已不存在），`migrate_legacy_plugin_keys` 走结构体并落盘。

- [ ] **Step 1：写失败测试（三条）**

① `src-tauri/src/plugins/mod.rs` 的 `mod tests` 内，把现有的
`迁移_非edge引擎不动`（L149-162，它用 `{"tts_engine":"mimo",…}` 做「不动」样本，
**加了 mimo 迁移后必然失败，必须一并改造**）整段替换为：

```rust
    #[test]
    fn 迁移_无关引擎不动() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"edge-tts","edge_voice":"x"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "edge-tts", "已是插件 id 的引擎不应改动");
        assert_eq!(v["edge_voice"], "x", "非 edge 旧标识的遗留字段不应被顺手清掉");
    }

    #[test]
    fn 迁移_mimo引擎改插件id并搬音色() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":"冰糖"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["tts_engine"], "mimo-tts");
        assert_eq!(v["plugin_voices"]["mimo-tts"], "冰糖", "旧 tts_model 应搬进插件音色表");
        assert!(v.get("tts_model").is_none(), "旧字段应被清除");
    }

    #[test]
    fn 迁移_mimo默认音色落到插件自述id() {
        let dir = tempfile::tempdir().unwrap();
        // 老版本 tts_model 的空串/"default" 都表示「用引擎默认音色」
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"tts_engine":"mimo","tts_model":"default"}"#,
        )
        .unwrap();

        migrate_legacy_engine(dir.path());

        let v = read_json(dir.path());
        assert_eq!(v["plugin_voices"]["mimo-tts"], "mimo_default");
    }
```

② `src-tauri/src/storage/settings.rs` 的 `mod tests` 内，`旧minimax键一次性迁移进plugin_config` 之后追加：

```rust
    #[test]
    fn 旧mimo键同时喂两个插件() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        // 真实状态：一个 MiMo Key 同时被 TTS 插件与 ASR 插件使用
        std::fs::write(
            d.join("settings.json"),
            r#"{"mimo_api_key":"km"}"#,
        )
        .unwrap();
        let s = load_settings(&d);
        assert_eq!(s.plugin_config["mimo-tts"]["api_key"], "km");
        assert_eq!(s.plugin_config["mimo-asr"]["api_key"], "km", "同一旧键映射多个插件时都要搬入");
        assert!(s.mimo_api_key.is_empty(), "旧键应清空");
    }

    #[test]
    fn 旧mimo键不覆盖已填配置() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        std::fs::write(
            d.join("settings.json"),
            r#"{"mimo_api_key":"old","plugin_config":{"mimo-tts":{"api_key":"new"}}}"#,
        )
        .unwrap();
        let s = load_settings(&d);
        assert_eq!(s.plugin_config["mimo-tts"]["api_key"], "new", "用户已填的不覆盖");
        assert_eq!(s.plugin_config["mimo-asr"]["api_key"], "old", "没填过的仍搬入");
        assert!(s.mimo_api_key.is_empty());
    }
```

- [ ] **Step 2：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib 迁移_
```
预期：`迁移_无关引擎不动` 通过、`迁移_mimo引擎改插件id并搬音色` 与
`迁移_mimo默认音色落到插件自述id` FAIL（当前函数见到非 `edge` 直接 return）。
再跑 `cargo test --lib 旧mimo` 预期两条都 FAIL（旧键还没进映射表）。

- [ ] **Step 3：`plugins/mod.rs` 迁移函数改多步**

整段替换 L29-70：

```rust
/// 兼容迁移：内置引擎抽出为插件后，把老设置里的一次性改写落盘（幂等，每次启动执行）：
/// - `tts_engine == "edge"` → `"edge-tts"`，`edge_voice` → `plugin_voices["edge-tts"]`
/// - `tts_engine == "mimo"` → `"mimo-tts"`，`tts_model` → `plugin_voices["mimo-tts"]`
///
/// 直接改 settings.json 文件（不经过 Settings 结构体——新结构体已无这些旧字段）。
/// 注意：默认值改 `edge-tts` 只影响全新安装与缺字段的情况，**不会把老用户悄悄换引擎**。
pub fn migrate_legacy_engine(data_dir: &Path) {
    let path = data_dir.join("settings.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut value: serde_json::Value = match serde_json::from_str(raw.trim_start_matches('\u{FEFF}')) {
        Ok(v) => v,
        Err(_) => return,
    };
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    let mut changed = false;

    // 1) 内置 edge → edge-tts 插件（带旧音色，缺失用默认晓晓）
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("edge") {
        let voice = obj
            .get("edge_voice")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("zh-CN-XiaoxiaoNeural")
            .to_string();
        obj.insert("tts_engine".into(), serde_json::Value::String("edge-tts".into()));
        obj.remove("edge_voice");
        set_plugin_voice_if_absent(obj, "edge-tts", voice);
        changed = true;
    }

    // 2) 内置 mimo → mimo-tts 插件（旧 tts_model 值即音色 id；空/"default" → 插件自述默认音色）
    if obj.get("tts_engine").and_then(|x| x.as_str()) == Some("mimo") {
        let voice = obj
            .get("tts_model")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && *s != "default")
            .unwrap_or("mimo_default")
            .to_string();
        obj.insert("tts_engine".into(), serde_json::Value::String("mimo-tts".into()));
        obj.remove("tts_model");
        set_plugin_voice_if_absent(obj, "mimo-tts", voice);
        changed = true;
    }

    if !changed {
        return;
    }
    if let Err(e) = crate::storage::atomic::write_json_pretty(&path, &value) {
        log_warn!("引擎标识迁移设置保存失败: {e}");
    }
}

/// 写入 `plugin_voices[plugin_id]`：已有值则保留（不覆盖用户在新面板里的选择）。
fn set_plugin_voice_if_absent(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    plugin_id: &str,
    voice: String,
) {
    let pv = obj
        .entry("plugin_voices")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = pv.as_object_mut() {
        map.entry(plugin_id.to_string())
            .or_insert(serde_json::Value::String(voice));
    }
}
```

> 原来的 `if … != Some("edge") { return; }` 早退结构必须拆开：两个 `if` 独立判断，
> 谁都不挡谁。用 `changed` 标志收口写盘，避免「没改动也重写文件」造成的
> mtime 抖动（会触发 `settings:changed` 无谓广播）。

- [ ] **Step 4：`storage/settings.rs` Key 迁移加两行 + 修「一 Key 多插件」的清空 bug**

① L107-114 映射表：

```rust
/// 旧硬编码插件 Key → plugin_config 的迁移映射。
/// 一次性历史包袱清理代码。国内版插件已由 minimax-tts（整段）升级为
/// minimax-tts-stream（流式，v0.3.0 起国内整段版已从内置移除），故旧国内键平移到流式插件。
/// 一个旧键可以映射多个插件（MiMo Key 同时喂 TTS 与 ASR），此时逐条搬同一份快照值。
const LEGACY_PLUGIN_KEY_MAP: &[(&str, &str, &str)] = &[
    // (settings 旧键, 插件 id, plugin_config 字段 key)
    ("minimax_api_key", "minimax-tts-stream", "api_key"),
    ("minimax_global_api_key", "minimax-tts-global", "api_key"),
    ("mimo_api_key", "mimo-tts", "api_key"),
    ("mimo_api_key", "mimo-asr", "api_key"),
];
```

② L119-150 函数体：读旧值必须走**快照**（否则第一条把 `s.mimo_api_key` 清空后，
第二条读到空值直接 `continue`，mimo-asr 永远搬不进去——这是本 Task 唯一的真陷阱）：

```rust
fn migrate_legacy_plugin_keys(s: &mut Settings) -> bool {
    let mut changed = false;
    // 快照：同一旧键映射到多个插件时，逐条都用原始值搬运（不能在循环里读已被清空的字段）
    let snapshot = s.clone();
    for (legacy_key, plugin_id, field_key) in LEGACY_PLUGIN_KEY_MAP {
        let legacy_value = match *legacy_key {
            "minimax_api_key" => snapshot.minimax_api_key.clone(),
            "minimax_global_api_key" => snapshot.minimax_global_api_key.clone(),
            "mimo_api_key" => snapshot.mimo_api_key.clone(),
            _ => String::new(),
        };
        if legacy_value.is_empty() {
            continue;
        }
        let empty_here = s
            .plugin_config
            .get(*plugin_id)
            .and_then(|m| m.get(*field_key))
            .map_or(true, |v| v.is_empty());
        if empty_here {
            s.plugin_config
                .entry(plugin_id.to_string())
                .or_default()
                .insert(field_key.to_string(), legacy_value);
        }
        // 无论是否搬入（新位置已有值时旧值作废），旧键都清空
        match *legacy_key {
            "minimax_api_key" => s.minimax_api_key = String::new(),
            "minimax_global_api_key" => s.minimax_global_api_key = String::new(),
            "mimo_api_key" => s.mimo_api_key = String::new(),
            _ => {}
        }
        changed = true;
    }
    changed
}
```

> `Settings` 派生了 `Clone`（`storage/types.rs:48`），快照一行成本可忽略；
> 迁移只在 `load_settings` 里跑一次，不在热路径。
> `mimo_api_key` 字段**本版本保留**（照 MiniMax 先例分两步走，见设计 §3.4-4），
> 下一版本再删字段与这些历史包袱。

- [ ] **Step 5：跑测试确认通过**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib
```
预期：**166 passed; 0 failed; 3 ignored**（161 + plugins/mod.rs 净增 3 + settings.rs 净增 2）。

- [ ] **Step 6：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src-tauri/src/plugins/mod.rs src-tauri/src/storage/settings.rs
git commit -m "feat(迁移): mimo→mimo-tts 三段一次性迁移（引擎标识/tts_model/遗留 Key）

- migrate_legacy_engine 由单步早退改为多步独立判断 + changed 收口写盘
- 一个 MiMo Key 同时搬入 mimo-tts 与 mimo-asr：旧值走快照读，修掉搬完即清空导致
  第二条映射读空的老陷阱
- 原「非 edge 引擎不动」测试样本改用已迁移完的 edge-tts（mimo 已是需要迁移的标识）

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8：克隆样本命令去引擎化 + 卸载兜底

**Files:**
- Modify: `src-tauri/src/commands/clone_voice.rs`（20MB 通用防呆 / 5 种扩展名 / `VA_CLONE_SAMPLE` 实时通告）
- Modify: `src-tauri/src/lib.rs`（Step 5 的启动注入块改调 helper）
- Modify: `src-tauri/src/commands/plugins.rs`（`uninstall_plugin` 末尾接兜底 + 纯函数 `pick_fallback_engine`）

> 设计 §3.2-3 与 §二-5。**宿主不再知道任何引擎的样本限制**：上限改通用防呆，
> 精确校验交插件（Task 1 的 `CLONE_MAX_BYTES`）；哪个引擎用样本由 `VA_CLONE_SAMPLE`
> 这个通用 env 传达，格式/用法插件自决。
> 卸载兜底的判据只用「能力位」：已加载 + 无必填配置项 → 免配置开箱即用。
> **不写 `== "edge-tts"`**（那会把引擎身份装回基座）。

- [ ] **Step 1：写失败测试（兜底选择纯函数 + 样本 env 契约）**

在 `src-tauri/src/commands/plugins.rs` 的 `mod tests` 内追加（若无 `mod tests`，
在文件末尾新建 `#[cfg(test)] mod tests { use super::*; … }`）：

```rust
    /// 造一条插件视图：loaded 决定是否可用，required 决定是否「需要填配置」
    fn info(id: &str, loaded: bool, plugin_type: &str, required: bool) -> PluginInfo {
        let config = if required {
            Some(PluginConfigDecl {
                help_url: None,
                fields: vec![PluginConfigField {
                    key: "api_key".into(),
                    r#type: "secret".into(),
                    label: "API Key".into(),
                    description: String::new(),
                    placeholder: String::new(),
                    env: format!("VA_TEST_{id}_KEY").to_uppercase().replace('-', "_"),
                    required: true,
                    options: None,
                }],
            })
        } else {
            None
        };
        PluginInfo {
            id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            description: String::new(),
            loaded,
            error: None,
            voices: vec![],
            audio_format: "wav".into(),
            path: String::new(),
            plugin_type: plugin_type.into(),
            category: "remote".into(),
            has_setup: false,
            setup_status: None,
            has_voice_management: false,
            requirements: None,
            config,
        }
    }

    #[test]
    fn 兜底引擎_优先免配置的可用插件() {
        let list = vec![
            info("needs-key", true, "tts_engine", true),
            info("free-but-slower", true, "tts_engine", false),
            info("not-loaded", false, "tts_engine", false),
        ];
        assert_eq!(pick_fallback_engine(&list).as_deref(), Some("free-but-slower"));
    }

    #[test]
    fn 兜底引擎_同优先级按id字典序() {
        let list = vec![
            info("zzz-free", true, "tts_engine", false),
            info("aaa-free", true, "tts_engine", false),
        ];
        assert_eq!(pick_fallback_engine(&list).as_deref(), Some("aaa-free"), "结果须确定性");
    }

    #[test]
    fn 兜底引擎_忽略非TTS插件与未加载项() {
        let list = vec![
            info("some-asr", true, "asr_engine", false),
            info("broken", false, "tts_engine", false),
        ];
        assert_eq!(pick_fallback_engine(&list), None, "一个可用引擎都没有时不改用户设置");
    }

    #[test]
    fn 样本env设置与移除() {
        // 设计 §六 的「导入后 env 存在、删除后 env 移除」：helper 是纯 env 副作用，
        // 直接给它的契约定测试（命令本身需 AppHandle/State，不适合单测）。
        // 全仓只有本测试读写 VA_CLONE_SAMPLE，并行跑不会互相干扰。
        crate::commands::clone_voice::announce_clone_sample(std::path::Path::new("C:/va-test/clone.wav"));
        assert_eq!(std::env::var("VA_CLONE_SAMPLE").unwrap(), "C:/va-test/clone.wav");
        crate::commands::clone_voice::clear_clone_sample();
        assert!(std::env::var("VA_CLONE_SAMPLE").is_err(), "删除样本后必须撤销告知");
    }
```

> import 需要 `use crate::plugins::manifest::{PluginConfigDecl, PluginConfigField};`：
> `PluginConfigField` 的字段名是 `r#type`（`manifest.rs` 里 `type` 是保留字），
> 照 `plugins/config.rs:77-88` 的既有写法抄。

- [ ] **Step 2：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib 兜底引擎
```
预期：编译失败——`cannot find function pick_fallback_engine in this scope`（三条测试同时红）；
补上 helper 前还会报 `cannot find function announce_clone_sample in module crate::commands::clone_voice`（第四条测试）。

- [ ] **Step 3：`commands/plugins.rs` 加纯函数并接到卸载命令末尾**

① 在 `uninstall_plugin` 之前插入纯函数：

```rust
/// 当前引擎被卸载后的兜底选择（判据只看能力，不看引擎身份）：
/// 1. 只在「已加载 + tts_engine 类型」里挑；
/// 2. 优先没有必填配置项的（= 免配置开箱即用，如免费云端引擎；需 Key 的排在后面）；
/// 3. 同优先级按 id 字典序，保证同一状态每次都给出同一个答案。
/// 返回 None = 一个可用引擎都没有，调用方应保持用户原值并给出指引文案。
pub(crate) fn pick_fallback_engine(list: &[PluginInfo]) -> Option<String> {
    let needs_config = |p: &PluginInfo| {
        p.config
            .as_ref()
            .map(|c| c.fields.iter().any(|f| f.required))
            .unwrap_or(false)
    };
    let mut usable: Vec<&PluginInfo> = list
        .iter()
        .filter(|p| p.loaded && p.plugin_type == "tts_engine")
        .collect();
    if usable.is_empty() {
        return None;
    }
    usable.sort_by(|a, b| needs_config(a).cmp(&needs_config(b)).then_with(|| a.id.cmp(&b.id)));
    Some(usable[0].id.clone())
}
```

② `uninstall_plugin` 的最后一行 `plugins.uninstall(&id).map_err(|e| e.to_string())` 替换为：

```rust
    let outcome = plugins.uninstall(&id).map_err(|e| e.to_string())?;

    // 卸载兜底：被卸的正是当前引擎时自动切到可用引擎，避免用户下次合成撞「引擎不可用」。
    // registry 已在 uninstall 里移除该条目，list() 天然不会再选中它。
    let was_current = state
        .settings
        .read()
        .map(|s| s.tts_engine == id)
        .unwrap_or(false);
    let notice = if was_current {
        let list = plugins.list();
        match pick_fallback_engine(&list) {
            Some(next) => {
                let next_name = list
                    .iter()
                    .find(|p| p.id == next)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| next.clone());
                let updated = crate::storage::settings::update_setting(
                    &state.data_dir,
                    "tts_engine",
                    serde_json::json!(next),
                )
                .map_err(|e| format!("{e}"))?;
                if let Ok(mut g) = state.settings.write() {
                    *g = updated;
                }
                notify_changed(&app, EVENT_SETTINGS_CHANGED);
                Some(format!("当前引擎已卸载，语音合成已自动切换到「{next_name}」。"))
            }
            None => Some(
                "当前引擎已卸载且没有可用的替代引擎：请到「插件」页安装任一语音合成引擎，\
                 或在语音中心切换默认引擎。"
                    .to_string(),
            ),
        }
    } else {
        None
    };

    Ok(match notice {
        Some(msg) => format!("{outcome} {msg}"),
        None => outcome,
    })
```

> `notify_changed(&app, EVENT_SETTINGS_CHANGED)` 与 `app`/`state` 都在现有命令签名里
> （L32-37），无需改参数。`update_setting` 走磁盘 → 返回完整 Settings 再刷内存，
> 与 `uninstall_plugin` 前半段清 `plugin_config` 的写法一致（避免多实例并发覆盖）。

- [ ] **Step 4：`clone_voice.rs` 去引擎化 + 实时通告样本**

① 文件头与大小/格式校验（L1-34 区间的两处判断）替换：

```rust
// 克隆音色样本导入/删除命令：宿主只管「一份通用样本文件」的存储与告知，
// 哪个引擎用它、支持哪些格式、多大上限，全部由插件自己判定（基座不认引擎身份）。
// 首版只支持一个克隆样本。

use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use crate::commands::AppState;
use crate::sync::{notify_changed, EVENT_SETTINGS_CHANGED};

/// 样本绝对路径经通用环境变量 `VA_CLONE_SAMPLE` 告知插件（与 API Key 同一路数：
/// 插件运行期 std::env::var 读取，改完即生效，无需重启）。
pub(crate) fn announce_clone_sample(abs_path: &Path) {
    std::env::set_var("VA_CLONE_SAMPLE", abs_path);
}

/// 样本删除后撤销告知：留一个指向已消失文件的 env 会让插件报「样本文件不存在」，
/// 不如直接 remove，让插件据此给出「请先导入样本」的文案。
pub(crate) fn clear_clone_sample() {
    std::env::remove_var("VA_CLONE_SAMPLE");
}

/// 导入克隆音色样本：复制本地文件到 voice_samples/ 下，
/// 并把样本路径与用户起的名字写进 settings。
#[tauri::command]
pub fn import_clone_voice(
    file_path: String,
    name: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let data_dir = &state.data_dir;
    let src = Path::new(&file_path);

    // 通用防呆上限 20MB：宿主不检查引擎各自的限制（如某云端引擎要求 ≤10MB），
    // 精确校验由插件在合成时给出中文原因，避免把引擎知识写进基座。
    let meta = std::fs::metadata(src).map_err(|e| format!("读取样本文件失败: {e}"))?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("样本文件过大（上限 20MB），请截取更短的音频".into());
    }

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "mp3".into());
    // 常见音频容器一律放行：能不能真用由插件判断（它才知道自家引擎支持哪些 MIME）
    if !matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "flac" | "ogg") {
        return Err("样本仅支持 mp3 / wav / m4a / flac / ogg 格式".into());
    }
```

② 在 `import_clone_voice` 的成功收尾处（`notify_changed(&app, EVENT_SETTINGS_CHANGED);` 之前）插一行：

```rust
    announce_clone_sample(&dest); // dest 是上面复制出来的样本绝对路径
```

③ `remove_clone_voice` 的成功收尾处（同一位置）插一行：

```rust
    clear_clone_sample();
```

- [ ] **Step 5：`lib.rs` 的启动注入块改调 helper**

把 Task 5 Step 4 写入的 `VA_CLONE_SAMPLE` 注入块收敛到同一实现（避免两处各写一份
「文件不存在就不注入」的判断）。整块替换为：

```rust
            // 克隆样本通用环境变量：宿主只把「已导入样本的绝对路径」告诉插件，
            // 格式/大小/用法全部由插件自决（基座不认识任何引擎，见 AGENTS.md 基座合同）。
            // API Key 不再在此硬编码注入：改由各插件 manifest 的 config 声明，
            // 统一走下方 plugin_manager.inject_config_env。
            {
                let sample_abs: Option<PathBuf> = if settings.clone_voice_path.is_empty() {
                    None
                } else {
                    let p = data_dir.join(&settings.clone_voice_path);
                    p.exists().then_some(p)
                };
                match sample_abs {
                    Some(p) => crate::commands::clone_voice::announce_clone_sample(&p),
                    None => crate::commands::clone_voice::clear_clone_sample(),
                }
            }
```

> `lib.rs` 若尚未 `use std::path::PathBuf;` 就加一行（以编译器报错为准，
> 零 warning 是 Task 13 门禁）。此处用 `PathBuf` 而非内联 `std::path::Path::new`，
> 是因为 helper 收 `&Path`、`PathBuf` 直接借用即可。

- [ ] **Step 6：编译与测试**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo check --lib --tests
cargo test --lib
pwsh -NoProfile -ExecutionPolicy Bypass -File ..\tools\check-bom.ps1
```
预期：零 warning；**170 passed; 0 failed; 3 ignored**（166 + Step 1 的 4 条）。

- [ ] **Step 7：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src-tauri/src/commands/clone_voice.rs src-tauri/src/commands/plugins.rs src-tauri/src/lib.rs
git commit -m "feat(基座): 克隆样本命令去引擎化 + 卸载当前引擎时自动兜底切换

- 样本上限改通用防呆 20MB、扩展名放宽到 5 种，精确校验交插件自述
- 导入/删除样本实时 set/remove VA_CLONE_SAMPLE，lib.rs 启动注入改调同一 helper
- 卸载兜底只按能力位选（已加载 + 无必填配置项优先 + id 字典序），不写引擎身份

Co-Authored-By: Claude <noreply@anthropic.com>"
```

## Task 9：流式播放态事件 + 桥路径改走流式

**Files:**
- Modify: `src-tauri/src/commands/tts_stream.rs`（抽 `generate_tts_stream_impl` + 播放线程内发事件）
- Modify: `src-tauri/src/plugins/bridge.rs:288-297`（`native_synthesize` 改走流式）

> 设计 §3.2-1（确定的现存缺陷）：今天流式播放时**悬浮球不动**（`FloatingBall.tsx:175`
> 听 `va:play:start`）、**手机遥控显示空闲**（`bridge.rs:93` 听 `playback:started` 聚合）。
> 两条硬约束：① `StreamPlayer::spawn()` 拿不到 `AppHandle`，故由 `BeginSpec` 携带（`AppHandle`
> 是 `Send + Sync`），事件在播放线程内发；② `bridge.rs` 把 `playback:started` 的 payload
> 按 `String`（音频相对路径）反序列化，**不是字符串就直接 `return`、聚合不生效**，
> 故必须带 `wav_rel`（`va:play:*` 无 payload，与前端写法一致）。
>
> **本 Task 与设计的两处有意收敛（执行时按本计划，Task 13 回写设计文档）**：
> 1. 事件只在 **`spk_ok` 为真**时发（设计 §3.2-1 写的是“扬声器或麦任一路开成”）。
>    理由：扬声器没开成时 `host_played=false`，前端会整段播并自己发一套事件；
>    宿主再发一套会把 `playback:stopped` 提前提给订阅方（遥控端会在前端还在播时显示空闲）。
> 2. `fid`/`wav_rel` 从原来的步骤 4（L191-195）**前移到开局之前**，因为 `playback:started`
>    需要它作为载荷。前移不改变行为：失败收尾本来就不落消息记录。
>
> **本 Task 不新增单测**：改动全部在 `rodio` 播放线程与 Tauri 事件连线上，
> 无纯函数可拆（拆个 `fn should_announce(b: bool) -> b` 是假测试）。
> 验证手段：编译零警告 + 现有测试不退化 + Task 13 人工验收清单里的两条可观察项
>（悬浮球起波纹 / 遥控端显示“正在播放”）。

- [ ] **Step 1：命令拆成 `impl`（桥路径复用）**

`src-tauri/src/commands/tts_stream.rs` L22 的 import 加 `Emitter`（发事件必须）：

```rust
use tauri::{AppHandle, Emitter, Manager};
```

L94-99（命令入口）替换为：

```rust
#[tauri::command]
pub async fn generate_tts_stream(text: String, app: AppHandle) -> Result<StreamTtsResult, String> {
    generate_tts_stream_impl(&app, &text).await
}

/// generate_tts_stream 核心实现（Tauri 命令与宿主能力桥共用）：
/// 导出流式符号的插件边合边播（`host_played=true`）；
/// 其余引擎内部回落阻塞管线（`host_played=false`，扬声器由前端整段播）。
pub async fn generate_tts_stream_impl(
    app: &AppHandle,
    raw_text: &str,
) -> Result<StreamTtsResult, String> {
    let text = raw_text.trim().to_string();
    if text.is_empty() {
        return Err("文本为空".into());
    }
```

函数体其余部分（原 L101-298）**逐字不动**，只按 Step 2/3 改开局与收尾。
原函数尾部的 `Ok(StreamTtsResult { message, host_played: spk_ok })` 保持。

- [ ] **Step 2：归档路径前移 + `BeginSpec` 携带句柄与路径**

原 L165-180（“3. 登记活跃会话 + 播放线程开局”）整段替换：

```rust
    // 3. 登记活跃会话 + 播放线程开局（回执设备打开结果）
    let _active_guard = ActiveGuard::new(Arc::clone(&plugin), session);

    // 归档路径先算：播放线程开局时要把「正在播哪个文件」告诉订阅方
    let fid = gen_id("m");
    std::fs::create_dir_all(data_dir.join("audio"))
        .map_err(|e| format!("创建音频目录失败: {e}"))?;
    let tmp_pcm = data_dir.join(format!("audio/{fid}.pcm.tmp"));
    let wav_rel = format!("audio/{fid}.wav");

    let (ack_tx, ack_rx) = channel();
    let has_mic_target = mic_send_enabled && !mic_device.is_empty();
    let mic_target = if has_mic_target {
        Some((mic_device, mic_volume.clamp(0.0, 1.0)))
    } else {
        None
    };
    app.state::<StreamPlayer>().begin(BeginSpec {
        sample_rate,
        channels,
        speaker_volume: speaker_volume.clamp(0.0, 1.0),
        mic: mic_target,
        ack: ack_tx,
        app: app.clone(),
        wav_rel: wav_rel.clone(),
    });
```

并**删掉**原 L190-195（现在已前移的三行）：

```rust
    // 4. 拉块循环（blocking 线程）：块 → 播放 + 追加临时 PCM 文件
    let fid = gen_id("m");
    std::fs::create_dir_all(data_dir.join("audio"))
        .map_err(|e| format!("创建音频目录失败: {e}"))?;
    let tmp_pcm = data_dir.join(format!("audio/{fid}.pcm.tmp"));
    let wav_rel = format!("audio/{fid}.wav");
```

→ 换成只保留小标题（后面那两行 `let engine_for_log` / `let text_len` 不动）：

```rust
    // 4. 拉块循环（blocking 线程）：块 → 播放 + 追加临时 PCM 文件
```

> 原步骤 6 的 `Message { id: gen_id("m"), … }`（L273）**保持不动**：
> 它又生了一个新 id（与文件名 `fid` 不同），属主线既有行为，本次不顺带改。

- [ ] **Step 3：`BeginSpec` / `PlaySession` / 播放线程事件收发**

① L361-371 的 `BeginSpec` 换为：

```rust
/// 开局参数（generate_tts_stream_impl → 播放线程）
pub struct BeginSpec {
    pub sample_rate: u32,
    pub channels: u16,
    /// 扬声器音量（settings.playback_volume）
    pub speaker_volume: f32,
    /// Some((设备名, 音量))：虚拟麦克风同源推流
    pub mic: Option<(String, f32)>,
    /// 开局回执：(扬声器已打开, 虚拟麦克风已打开)
    pub ack: Sender<(bool, bool)>,
    /// 应用句柄：`rodio::OutputStream` 不是 Send，但 `AppHandle` 是，
    /// 播放态事件就在这个线程里发
    pub app: AppHandle,
    /// 本次流式归档的音频相对路径（`playback:started` 的载荷，能力桥据此聚合播放态）
    pub wav_rel: String,
}
```

② L416-421 的 `PlaySession` 与紧随其后的 `announce_play_stop`：

```rust
struct PlaySession {
    sample_rate: u32,
    channels: u16,
    sinks: Vec<DeviceSink>,
    finishing: bool,
    app: AppHandle,
    /// 开局时宿主是否发过 `va:play:start`（决定收尾要不要发对应的那一套 stop）
    announced: bool,
}

/// 宿主播完/中断时的播放态收尾：只补发开局时发过 start 的那一套。
/// 前端整段播放路径自己会发一套，两边不发两套是 `announced` 标志存在的全部意义。
fn announce_play_stop(s: &PlaySession) {
    if !s.announced {
        return;
    }
    let _ = s.app.emit("va:play:stop", ());
    let _ = s.app.emit("playback:stopped", ());
}
```

③ `player_thread`（L423-499）四处改动：

- `Ok(PlayerCmd::Begin(spec)) => {` 开头的 `active = None;`（及其注释）换为：

```rust
                // 新流替换旧流（未播完的尾巴直接切断，与虚拟麦克风 Play 语义一致）
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
```

- 结尾的 `let _ = spec.ack.send((spk_ok, mic_ok));` 与 `if !sinks.is_empty() { … }` 整块换为：

```rust
                let _ = spec.ack.send((spk_ok, mic_ok));
                let app = spec.app.clone();
                let wav_rel = spec.wav_rel.clone();
                // 只在扬声器真开成时发：开不成时 host_played=false，
                // 前端会整段播放并自行发一套事件（两边都发会双计）
                let announced = spk_ok;
                if announced {
                    let _ = app.emit("va:play:start", ());
                    let _ = app.emit("playback:started", wav_rel.clone());
                }
                if !sinks.is_empty() {
                    active = Some(PlaySession {
                        sample_rate: spec.sample_rate,
                        channels: spec.channels,
                        sinks,
                        finishing: false,
                        app,
                        announced,
                    });
                }
```

- `Ok(PlayerCmd::Abort) => active = None,` 换为：

```rust
            Ok(PlayerCmd::Abort) => {
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
            }
```

- `Err(RecvTimeoutError::Timeout)` 与 `Err(…Disconnected)` 两个分支换为：

```rust
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let drained = active
                    .as_mut()
                    .map(|s| s.finishing && s.sinks.iter().all(|ds| ds.sink.empty()))
                    .unwrap_or(false);
                if drained {
                    // 排空完成：先报停止再释放设备，订阅方不会看到「播完还在播」
                    if let Some(prev) = active.take() {
                        announce_play_stop(&prev);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // 发送端已全部 drop（主命令走到尽头）：补发停止事件后退出
                if let Some(prev) = active.take() {
                    announce_play_stop(&prev);
                }
                break;
            }
```

> 原代码在 `if let Some(s) = &mut active { … active = None; }` 里直接赋值，
> 改成 `take()` 是为了先拿所有权再发事件（避开借用冲突，也更好读）。

- [ ] **Step 4：桥路径改走流式**

`src-tauri/src/plugins/bridge.rs` L288-297 整段替换：

```rust
/// 合成并播放（async，供本体遥控直接 await）：统一走流式管线。
/// - 插件导出流式三符号 → 宿主边合边播（已发播放态事件），不再 emit `playback:play`；
/// - 其余引擎 → 内部回落阻塞管线（`host_played=false`），仍由主窗播扬声器。
/// 与后端麦克风避免双份（两条管线内部各自按需发麦克风）。
pub async fn native_synthesize(app: &AppHandle, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("文本不能为空".to_string());
    }
    let res = crate::commands::tts_stream::generate_tts_stream_impl(app, text).await?;
    if !res.host_played {
        let _ = app.emit("playback:play", res.message.audio_path);
    }
    Ok(())
}
```

> 这是设计 §七 里“改动面 +1、需一并验收遥控”的那条：今天遥控发字是整段播，
> 本步后也是“先出声后结束”。`playback:play` 的载荷口径不变（音频相对路径）。

- [ ] **Step 5：编译与测试**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo check --lib --tests
cargo test --lib
```
预期：零 warning（报 `cannot find function` 先确认 `generate_tts_stream_impl` 是 `pub`）；
**170 passed; 0 failed; 3 ignored**（本 Task 不增删测试）。

- [ ] **Step 6：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src-tauri/src/commands/tts_stream.rs src-tauri/src/plugins/bridge.rs
git commit -m "feat(流式): 播放线程内补齐播放态事件 + 能力桥合成改走流式管线

- BeginSpec 携带 AppHandle/wav_rel，开局成功时发 va:play:start + playback:started{路径}；
  抢占旧流 / Abort / 排空完成 / 发送端断开四处收尾均补发对应的 stop（仅当开局发过 start）
- 修现存缺陷：流式播放时悬浮球不动、遥控端显示空闲
- native_synthesize 改调 generate_tts_stream_impl：host_played=true 时不再 emit playback:play

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 10：远程配置按插件 id 覆盖 `help_url`（换邀请码免发版）

**Files:**
- Modify: `src-tauri/src/commands/remote.rs`（`RemoteConfig` 加 map + `effective_help_url` + 测试）
- Modify: `src-tauri/src/commands/plugins.rs:186`（`get_plugin_config` 走生效入口）
- Modify: `plugins/remote-config.json`（线上 JSON 加覆盖表）

> 设计 §二-4 / §3.2-4。定为 **map**（键 = 插件 id，值 = help_url）：比数组好合并、不会重键、
> 读不到就不动。
> **前端唯一生效点是 `get_plugin_config`**（已 grep 证实：`PluginConfigPanel.tsx:63` 读
> `info.help_url`，而 `list_plugins` 返回的 `PluginInfo.config.help_url` 前端无人读），
> 故只改一处；`list()` 里的 `config` 字段作为原始声明保留，不强行抹平。
> 同步命令里**不能发网络请求**：只读 24h 缓存（`read_cache`），没缓存就用 manifest 兜底。

- [ ] **Step 1：写失败测试（三条）**

在 `src-tauri/src/commands/remote.rs` 的 `mod tests` 内追加：

```rust
    use std::collections::HashMap;

    #[test]
    fn 远程配置解析plugin_help_url覆盖表() {
        let cfg: RemoteConfig = serde_json::from_str(
            r#"{"plugin_help_url":{"some-tts":"https://x.example.com?ref=AAA"}}"#,
        )
        .unwrap();
        assert_eq!(cfg.plugin_help_url["some-tts"], "https://x.example.com?ref=AAA");
    }

    #[test]
    fn 缺覆盖表时为空map不报错() {
        let cfg: RemoteConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.plugin_help_url.is_empty(), "老版本线上 JSON 没这个字段也要能解");
    }

    #[test]
    fn help_url_远程覆盖优先无覆盖退兜底() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        // 无缓存：直接退 manifest 兜底
        assert_eq!(
            effective_help_url(d, "mimo-tts", Some("https://fallback.example")),
            Some("https://fallback.example".to_string())
        );
        // 有覆盖表：命中则覆盖，未命中/空串仍退兜底
        let mut map = HashMap::new();
        map.insert("mimo-tts".to_string(), "https://remote.example?ref=ZZZ".to_string());
        map.insert("other-tts".to_string(), "   ".to_string());
        write_cache(
            d,
            &RemoteConfig {
                mimo_invite_code: DEFAULT_INVITE_CODE.to_string(),
                plugin_help_url: map,
            },
        );
        assert_eq!(
            effective_help_url(d, "mimo-tts", Some("https://fallback.example")),
            Some("https://remote.example?ref=ZZZ".to_string()),
            "换码不应要求发版"
        );
        assert_eq!(
            effective_help_url(d, "not-in-map", Some("https://fallback.example")),
            Some("https://fallback.example".to_string())
        );
        assert_eq!(
            effective_help_url(d, "other-tts", None),
            None,
            "空串覆盖不算覆盖"
        );
    }
```

- [ ] **Step 2：跑测试确认失败**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib help_url
```
预期：编译失败——`no field 'plugin_help_url'` / `cannot find function effective_help_url`。

- [ ] **Step 3：`remote.rs` 字段与生效函数**

① L6-8 的 import 补一行（结构体字段用 `HashMap`）：

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
```

② L29-45 的 `RemoteConfig` 与 `Default` 实现换为：

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_invite_code")]
    pub mimo_invite_code: String,
    /// 按插件 id 覆盖 manifest 里的 `help_url`（邀请返利链接等运营数据）。
    /// 换码只改云端 JSON，不发版；缺省 = 不覆盖，用插件自带的静态兜底。
    #[serde(default)]
    pub plugin_help_url: HashMap<String, String>,
}

fn default_invite_code() -> String {
    DEFAULT_INVITE_CODE.to_string()
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            mimo_invite_code: default_invite_code(),
            plugin_help_url: HashMap::new(),
        }
    }
}
```

③ L58-61 的 `read_cache` 提高可见性并注因（`plugins.rs` 要复用）：

```rust
/// 读本地缓存（同步、不发网络）
// pub(crate)：commands/plugins.rs 的 get_plugin_config 需要据此覆盖 help_url
pub(crate) fn read_cache(data_dir: &Path) -> Option<CacheFile> {
    let raw = std::fs::read_to_string(cache_path(data_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}
```

④ `CacheFile`（L48-52）也需跟着提可见性：

```rust
/// 本地缓存文件结构（fetched_at 为 Unix 秒）
// pub(crate)：随 read_cache 一起对外可见（返回类型不能比函数更私有）
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CacheFile {
    fetched_at: u64,
    data: RemoteConfig,
}

/// 缓存里的 help_url 覆盖表（`data` 保持私有，只把表递出去）
impl CacheFile {
    pub(crate) fn help_url_override(&self, plugin_id: &str) -> Option<String> {
        self.data
            .plugin_help_url
            .get(plugin_id)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}
```

⑤ 在 `get_remote_config_inner`（L118）之前插入生效入口：

```rust
/// `help_url` 生效值：本地缓存里的远程覆盖表优先，否则退回 manifest 静态兜底。
/// 只读缓存、不发网络请求（调用方是同步命令）；没缓存/没拉到覆盖 = 不覆盖。
/// 设计见 doc/MiMo插件化与流式默认设计.md §3.2-4：保住「不发版就能换邀请码」。
pub(crate) fn effective_help_url(
    data_dir: &Path,
    plugin_id: &str,
    fallback: Option<&str>,
) -> Option<String> {
    if let Some(cache) = read_cache(data_dir) {
        if let Some(url) = cache.help_url_override(plugin_id) {
            return Some(url);
        }
    }
    fallback.map(|s| s.to_string())
}
```

- [ ] **Step 4：`get_plugin_config` 接上生效入口**

`src-tauri/src/commands/plugins.rs` L184-188 的返回换为：

```rust
    Ok(PluginConfigInfo {
        id: id.clone(),
        // help_url 按插件 id 走远程覆盖（运营链接不发版就能换），
        // 没覆盖时退回 manifest 静态值。前端只消费这一处（PluginConfigPanel）。
        help_url: crate::commands::remote::effective_help_url(
            &state.data_dir,
            &id,
            decl.help_url.as_deref(),
        ),
        fields,
    })
```

> `PluginConfigInfo { id, … }` 原来是 `id` 直接被 move；上面改成 `id.clone()`
> 是因为 `&id` 在后面调 `effective_help_url` 还要用。顺序上先借后用，
> 编译器会直接提醒，别提前删 `.clone()`。

- [ ] **Step 5：线上 JSON 加覆盖表**

`plugins/remote-config.json` 整文件替换（用无 BOM 的 UTF-8 写，见 AGENTS.md 陷阱表）：

```json
{
  "mimo_invite_code": "5P9J2B",
  "plugin_help_url": {
    "mimo-tts": "https://platform.xiaomimimo.com?ref=5P9J2B",
    "mimo-asr": "https://platform.xiaomimimo.com?ref=5P9J2B"
  }
}
```

> `mimo_invite_code` **字段保留不删**（`VoiceSynthPanel` 删掉邀请码卡前，老版本宿主
> 还在读它；Task 11 删前端消费点后它就成了只读兼容字段，下一版再清）。
> 两个插件共用同一个邀请链接，故两条都写：只写 `mimo-tts` 会导致 ASR 配置卡
> 退回 manifest 兜底（当前值相同，但换码时会不一致）。

- [ ] **Step 6：跑测试确认通过**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo check --lib --tests
cargo test --lib
```
预期：零 warning；**173 passed; 0 failed; 3 ignored**（170 + Step 1 的 3 条）。
若有 `missing field plugin_help_url` 编译错 → 是 L311 那个用字面量构 `RemoteConfig`
的测试，补上字段（Step 3② 已改结构体，该测试必须同步）：

```rust
        let cfg = RemoteConfig {
            mimo_invite_code: "CACHED".to_string(),
            plugin_help_url: std::collections::HashMap::new(),
        };
```

- [ ] **Step 7：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src-tauri/src/commands/remote.rs src-tauri/src/commands/plugins.rs plugins/remote-config.json
git commit -m "feat(远程配置): 新增按插件 id 覆盖 help_url（换邀请码链接免发版）

- RemoteConfig 加 plugin_help_url map（缺字段解为空 map，老 JSON 兼容）
- effective_help_url 只读本地缓存不发网络；命中非空覆盖值则优先 manifest 兜底
- get_plugin_config（前端唯一 help_url 消费点）接生效入口

Co-Authored-By: Claude <noreply@anthropic.com>"
```

## Task 11：前端去 MiMo 化（删引擎卡 / 下拉动态 / 克隆卡改由 voices 驱动）

**Files:**
- Delete: `src/components/Settings/ApiKeyModal.tsx`（已 grep 证实：**全仓无人 import**，死代码）
- Create: `src/components/Voice/CloneSamplePanel.tsx`
- Modify: `src/components/Voice/VoiceSynthPanel.tsx`（主战场）
- Modify: `src/App.tsx:144`、`src/services/invoke.ts:237-239`
- Modify: 文案 6 处（`VoiceInputButton.tsx:87`、`VoiceInputSettings.tsx:161`、`SubtitlePage.tsx:309`、`VoiceRecognizePanel.tsx:27`、`settingsStore.ts:2`、`SettingsSection.tsx:95`）

> 替代 UI 全部现成（设计 §3.5）：Key 走 `PluginConfigPanel`（manifest 驱动 + 自动保存 +
> `help_url` 链接），音色走 `plugin.voices` → `plugin_voices[引擎]`（于云引擎即开即用）。
> 邀请码复制卡随 MiMo 卡一起删：链接里已带 `?ref=`，而它已是“插件自述 + 远程覆盖”的数据，
> 基座前端不该再为某个引擎摆一个复制按钮。
> **`tsconfig.json` 开了 `noUnusedLocals` + `noUnusedParameters`**：删代码时残留的
> import / 局部变量会让 `npx tsc --noEmit` 直接红，本 Task 的护栏就是它。

- [ ] **Step 1：新建 `src/components/Voice/CloneSamplePanel.tsx`**

完整内容（从 `VoiceSynthPanel.tsx` L245-274 的 `onImportClone` / `onRemoveClone` 改造而来，
去掉所有 MiMo 字号）：

```tsx
// 克隆音色样本卡：显示与否由插件自述（`voices` 里含 id="clone" 的项就渲染）。
// 前端不认识任何引擎：支持哪些格式、大小上限、克隆怎么生效，全由插件判定；
// 音色名里的提醒（如「暂不支持低延迟」）也来自插件的 voices.label，前端只渲染不改写。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { getSettings, importCloneVoice, pickAudioFile, removeCloneVoice } from "../../services/invoke";
import { SubPanel } from "../common/SettingsSection";
import { toast } from "../common/Toast";

export function CloneSamplePanel({
  selected,
  onImported,
}: {
  /** 当前是否已选用克隆音色 */
  selected: boolean;
  /** 导入成功后把当前音色切到克隆（切法由父组件按通用机制决定） */
  onImported: () => void;
}) {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [importing, setImporting] = useState(false);

  async function onImport() {
    const filePath = await pickAudioFile();
    if (!filePath) return;
    const name = window.prompt(
      "给这个克隆音色起个名字：",
      settings?.clone_voice_name || "我的音色",
    );
    if (!name || !name.trim()) return;
    setImporting(true);
    try {
      await importCloneVoice(filePath, name.trim());
      onImported();
      // 后端已改写 clone_voice_path/name，重拉一次保证上屏
      setSettings(await getSettings());
    } catch (e) {
      toast(`导入克隆样本失败：${e}`, "err");
    } finally {
      setImporting(false);
    }
  }

  async function onRemove() {
    if (!window.confirm("确认删除克隆音色样本？")) return;
    try {
      await removeCloneVoice();
      setSettings(await getSettings());
    } catch (e) {
      toast(`删除失败：${e}`, "err");
    }
  }

  return (
    <SubPanel
      title="音色克隆"
      desc="导入一段 5–10 秒的本地说话音频作为克隆样本。支持的格式与大小上限由当前引擎自行判定；用克隆音色合成时需随请求上传样本，首响比预置音色慢。"
    >
      {settings?.clone_voice_path ? (
        <div className="space-y-2">
          <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--ink-100)]/50 px-3 py-2 text-xs text-[var(--ink-500)]">
            当前样本：<span className="font-medium text-[var(--ink-900)]">{settings.clone_voice_name || "未命名"}</span>
            {selected && (
              <span className="ml-2 text-[var(--amber-600)]">（已选用）</span>
            )}
          </div>
          <button
            onClick={onImport}
            disabled={importing}
            className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-xs text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] transition-colors disabled:opacity-50"
          >
            {importing ? "导入中…" : "替换样本"}
          </button>
          <button
            onClick={onRemove}
            className="w-full rounded-xl px-3 py-2 text-xs text-[var(--seal)] hover:bg-[var(--seal)]/10 transition-colors"
          >
            删除克隆样本
          </button>
        </div>
      ) : (
        <button
          onClick={onImport}
          disabled={importing}
          className="w-full rounded-xl bg-[var(--ink-900)] px-3 py-2 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
        >
          {importing ? "导入中…" : "+ 导入样本"}
        </button>
      )}
    </SubPanel>
  );
}
```

- [ ] **Step 2：`VoiceSynthPanel.tsx` 删 MiMo 卡与相关代码**

逐处改（行号以当前文件为准，**从上往下改会导致后面的行号前移，建议按“先大块后小块”顺序动手**）：

① L1-3 文件头注释：

```tsx
// 语音中心 · TTS 合成面板：默认引擎选择 + 逐引擎配置（内置 MOSS / 插件引擎）。
// 从设置页「语音合成」区整体析出，行为保持一致；MiniMax 国际克隆/管理交由 MinimaxVoicePanel 承担。
// 引擎以 EngineCard 同构呈现；本地引擎环境未就绪时仍走页内确认卡（在线下载 / 导入离线资源包），与原实现一致。
```

② L8-17 的 import 删 4 个（`importCloneVoice` / `removeCloneVoice` / `pickAudioFile` / `getRemoteConfig`），
并新增 `CloneSamplePanel`：

```tsx
import {
  listPlugins,
  preloadVoice,
  promptEngineWarmup,
  importResourcePackFlow,
} from "../../services/invoke";
import type { PluginInfo, MossVoice } from "../../types";
import { Field, SubPanel, SecretInput } from "../common/SettingsSection";
import { EngineCard } from "./EngineCard";
import { MinimaxVoicePanel } from "./MinimaxVoicePanel";
import { CloneSamplePanel } from "./CloneSamplePanel";
```

③ 删 L28-34 整段 `PRESET_VOICES` 常量。

④ 删 L39 的 `setSettings`（只服务于被删的克隆导入/删除函数；不删会触 `noUnusedLocals`），
并删 L41-52 整段（`importing` / `cloneName` / `copied` / `inviteCode` + 拉远程配置的 `useEffect`）。

⑤ L81 删 `currentVoice`（只被 MiMo 音色下拉用）；L83 改默认值：

```tsx
  const engineId = settings?.tts_engine ?? "edge-tts";
```

⑥ 删 L184-197 整段 `copyInvite`，删 L239-277 整段（`onPickVoice` / `onImportClone` /
`onRemoveClone` / `onSaveApiKey`）。

⑦ 引擎下拉删 `<option value="mimo">MiMo（小米）</option>` 一行（L288）；
`<option value="moss">` 与插件动态项保持原样。

⑧ 删 L298-392 整块 `{engineId === "mimo" && ( <EngineCard … MiMo API 密钥 / 音色管理 / 音色克隆 … > )}`。

⑨ L502 的守卫去掉 mimo 判断：

```tsx
      {engineId !== "moss" && (() => {
```

⑩ L504-525 的「引擎不可用」警示卡里，硬编码的“切回 MiMo”按钮改为按能力选替代引擎
（与 Task 8 的宿主兜底同口径：优先免配置，同优先级按 id 字典序）：

```tsx
        if (!cur) {
          // 当前引擎是插件但不可用：未安装（卸载/换机）或加载失败。给警示卡片 + 一键切到可用引擎
          const failed = plugins.find((p) => p.id === engineId);
          const needsKey = (p: PluginInfo) => !!p.config?.fields.some((f) => f.required);
          const alt = plugins
            .filter((p) => p.loaded && p.plugin_type === "tts_engine")
            .sort((a, b) => Number(needsKey(a)) - Number(needsKey(b)) || (a.id < b.id ? -1 : 1))[0];
          return (
            <div className="rounded-lg border border-[var(--seal)]/40 bg-[var(--seal)]/10 px-3 py-2.5">
              <p className="text-xs font-medium text-[var(--seal)]">
                {failed ? `当前引擎「${failed.name}」加载失败` : "当前引擎对应的插件未安装"}
              </p>
              <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
                {failed
                  ? `原因：${failed.error ?? "未知"}。可到插件管理页查看详情，或先切换到其他引擎。`
                  : `配置中记录的引擎「${engineId}」对应的插件尚未安装（可能已卸载或在别的设备上安装）。语音合成暂不可用，可先切到其他引擎，或到插件管理页安装。`}
              </p>
              {alt && (
                <button
                  onClick={() => handleEngineChange(alt.id)}
                  className="mt-2 rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)]"
                >
                  切换到「{alt.name}」
                </button>
              )}
            </div>
          );
        }
```

> “语音合成暂不可用，可先切回**内置**引擎”这句原文也已改掉：删完 MiMo 后
> 唯一内置的是 MOSS，而它也得填 Key，说“切回内置”会误导。

- [ ] **Step 3：在插件引擎卡里挂克隆样本卡（能力驱动）**

在 L577-590 那整块音色管理渲染（`MinimaxVoicePanel` / `VoiceManager` / `SubPanel` 三选一）**之后**、
“未安装音色：页内确认卡片”注释之前插入：

```tsx
            {/* 克隆样本卡：插件自述驱动——`voices` 里有 id="clone" 就显示，
                不往任何硬编码集合里加引擎名（基座合同 §3.6-1） */}
            {(() => {
              const cloneVoice = cur.voices.find((v) => v.id === "clone");
              if (!cloneVoice) return null;
              return (
                <CloneSamplePanel
                  selected={(settings?.plugin_voices?.[cur.id] ?? "") === cloneVoice.id}
                  onImported={() => handlePluginVoiceChange(cur, cloneVoice.id)}
                />
              );
            })()}
```

- [ ] **Step 4：全局残留清理（5 处，均为文案/默认值）**

① `src/App.tsx:144`：

```tsx
    const engineId = settings.tts_engine ?? "edge-tts";
```

② `src/services/invoke.ts` L237-239 的 `RemoteConfig` 接口加覆盖表（与后端字段同名）：

```ts
export interface RemoteConfig {
  mimo_invite_code: string;
  /** 按插件 id 覆盖 help_url（后端已消费；前端只作透传展示，无则用插件自带兜底） */
  plugin_help_url?: Record<string, string>;
}
```

③ 四处“（如 MiMo ASR）”举例文案改为不带引擎名（护栏会扫 `src/`，且 ASR 插件不止一家）：

- `src/components/Chat/VoiceInputButton.tsx:87` →
  `"请先到「插件库」安装语音识别插件，\n安装后在「语音」页将其设为当前识别引擎。",`
- `src/components/Settings/VoiceInputSettings.tsx:161` →
  `暂无可用的语音识别插件，请先在插件页安装。`
- `src/components/Subtitle/SubtitlePage.tsx:309` →
  `<Hint>暂无可用识别引擎，请先在「插件」页安装 ASR 插件。</Hint>`
- `src/components/Voice/VoiceRecognizePanel.tsx:27` →
  `暂无语音识别引擎。请到「插件」页安装 ASR 插件，装好后即可在此选择与测试。`

⑤ 两处注释去引擎名：

- `src/stores/settingsStore.ts:2` →
  `// 本阶段先放 settings（含各引擎 API Key），后续阶段加 messages/favorites。`
- `src/components/common/SettingsSection.tsx:95` →
  ` *  供内置引擎填写 API Key，与插件引擎的密钥填写观感统一。 */`

- [ ] **Step 5：删死代码 `ApiKeyModal.tsx`**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
Remove-Item .\src\components\Settings\ApiKeyModal.tsx
```

> 已 grep 证实全仓无 `import … ApiKeyModal`（只有自身定义行），直接删。

- [ ] **Step 6：类型与单测验证**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
npx tsc --noEmit
npm run test:run
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-bom.ps1
```
预期：`tsc` **零输出**（报 `declared but never used` 就是 Step 2 删漏了 import/局部变量；
报 `Cannot find name` 就是大块删到了正在被引用的函数）；
`vitest` 全绿（`settingsStore.test.ts` 的 `EXPECTED_KEYS` 仍含 `mimo_api_key`，
因为 `storage/types.rs` 的字段本版本保留——**不要删那三行**）；BOM 护栏通过。

- [ ] **Step 7：确认前端已无引擎身份残留**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
Get-ChildItem -Recurse -File -Include *.ts,*.tsx -Path src | Select-String -Pattern 'mimo' -CaseSensitive:$false | ForEach-Object { "$($_.Path):$($_.LineNumber): $($_.Line.Trim())" }
```
预期只剩四类“已挂号、本版本保留”的残留（Task 12 的基线清单就记在这里）：
- `src/types/index.ts:34`（`mimo_api_key` 字段，跟后端同命）
- `src/services/invoke.ts:238`（`mimo_invite_code`，只读兼容字段）
- `src/stores/settingsStore.test.ts` 的三处（字段存在性与 `asr_plugin: "mimo-asr"` 样本）

再查遥控端（设计 §3.6 残留清单最后一行“遥控端引擎列表”，护栏不扫 `remote-app/`）：

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
Get-ChildItem -Recurse -File -Include *.dart,*.js,*.ts -Path remote-app |
    Select-String -Pattern 'mimo|edge-tts|minimax|moss|hojo|genie' -CaseSensitive:$false
```
预期：**零输出**（写本计划时已实跑核对过：`remote-app/` 全部 .dart 无引擎字面量，
引擎列表本来就走 `list_plugins`，无需改动；真跑出东西则归到远程配置里改，不往本体贴补丁）。

- [ ] **Step 8：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add src/components/Voice/VoiceSynthPanel.tsx src/components/Voice/CloneSamplePanel.tsx src/components/Settings/ApiKeyModal.tsx src/App.tsx src/services/invoke.ts src/components/Chat/VoiceInputButton.tsx src/components/Settings/VoiceInputSettings.tsx src/components/Subtitle/SubtitlePage.tsx src/components/Voice/VoiceRecognizePanel.tsx src/stores/settingsStore.ts src/components/common/SettingsSection.tsx
git status --short
git commit -m "feat(前端): 语音中心删 MiMo 引擎卡，克隆样本卡改由插件 voices 驱动

- PRESET_VOICES / 邀请码卡 / MiMo EngineCard 整块删除，Key 与音色全部走现成的
  PluginConfigPanel + plugin.voices 通用机制
- 引擎下拉不再生成 mimo 项；「引擎不可用」的切回按钮改按能力选替代引擎（不写引擎名）
- 新增 CloneSamplePanel：渲染条件为 voices 含 id=clone，格式/上限判定权交插件
- 删除无引用的 ApiKeyModal.tsx；四处 ASR 举例文案去引擎名

Co-Authored-By: Claude <noreply@anthropic.com>"
```

## Task 12：基座护栏 `tools/check-base-hygiene.ps1` + CI + AGENTS.md 基座合同

**Files:**
- Create: `tools/check-base-hygiene.ps1`（含中文 → 必须带 BOM）
- Create（脚本产物，入 git）: `tools/base-hygiene-baseline.txt`
- Modify: `.github/workflows/consistency.yml`（第三个只读检查步骤）
- Modify: `AGENTS.md`（新增「基座合同」小节）

> 设计 §3.6：原则不落地成可执行检查就会腐化。护栏只扫本体两目录
> （`src-tauri/src`、`src`）——`plugins/` 下写引擎名字是天经地义的。
> **采用基线机制而不写死白名单**：存量残留（MOSS 内置、MiniMax 专供克隆命令、
> `GenieData` 路径判断等）实测有 70+ 条，本期拆完 MiMo 不可能一并拆完；
> 写成“只许变少不许变多”的基线才能今天上约、且不谎报进度。
> 已实测当前分布（供校对基线用）：`commands/plugins.rs|genie=31`、`plugins/manager.rs|genie=12`、
> `VoiceSynthPanel.tsx|moss=34`、`storage/settings.rs|minimax=39`、`tts/mimo.rs|mimo=34`（Task 6 应变 0）。

- [ ] **Step 1：写 `tools/check-base-hygiene.ps1`**

完整内容（惯用法照 `tools/check-log-hygiene.ps1`：头注释、`$ErrorActionPreference`、
`$repoRoot = Split-Path -Parent $PSScriptRoot`、`-Recurse -File -Filter`、`exit 1`、`✓/✗`）：

```powershell
# 基座护栏（AGENTS.md「基座合同」的机械化）：本体（src-tauri/src、src）不得
# 出现具体引擎的身份字面量。引擎差异只能以能力位/自述数据存在（manifest、
# voices、ABI 符号），写成 `if engine == "xxx"` 就是把某个引擎的知识装回基座。
#
# 用法：
#   pwsh tools/check-base-hygiene.ps1                  # 检查（CI 用）
#   pwsh tools/check-base-hygiene.ps1 -UpdateBaseline   # 重写基线（拆完残留后收紧）
#
# 基线文件 tools/base-hygiene-baseline.txt，每行 `相对路径|模式|允许次数`：
#   - 出现基线里没有的 (路径, 模式)        → 红
#   - 某条计数比基线大                    → 红（登记过的残留只许变少不许变多）
#   - 计数变小/条目消失                    → 绿，并打印“可收紧”提示
# 行内貁免：`// baseok: <理由>` 所在行不计（与 // allowlog: 同口径）。
# 注：plugins/ 目录不扫——插件里写自家引擎协议是它本该干的活。

param(
    [switch]$UpdateBaseline
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $PSScriptRoot 'base-hygiene-baseline.txt'

# 引擎身份字面量（大小写不敏感；xiaomimimo 被 mimo 覆盖，voiceclone 被 mimo 所在的
# 插件独有，仍单独列出以便单独计数）。**接入新引擎时先往这里加一条**，
# 于是本体里所有“写死了这个引擎”的地方当场变红——这就是本护栏的全部目的。
$patterns = @('mimo', 'minimax', 'hojo', 'genie', 'edge-tts', 'moss', 'voiceclone')

$scanRoots = @('src-tauri\src', 'src')
$fileGlobs = @('*.rs', '*.ts', '*.tsx')

$hits = @{}
$fileCount = 0
foreach ($root in $scanRoots) {
    $abs = Join-Path $repoRoot $root
    if (-not (Test-Path -LiteralPath $abs)) {
        Write-Error "找不到扫描目录：$abs"
    }
    foreach ($glob in $fileGlobs) {
        # 用 -Filter 而不是 -Include：后者与 -LiteralPath 组合在 PS 5.1 下不生效（踩过）
        $files = Get-ChildItem -LiteralPath $abs -Recurse -File -Filter $glob
        foreach ($f in $files) {
            $fileCount++
            $rel = $f.FullName.Substring($repoRoot.Length + 1) -replace '\\', '/'
            $text = [System.IO.File]::ReadAllText($f.FullName)
            foreach ($line in ($text -split "`r?`n")) {
                if ($line -match '//\s*baseok:') { continue }
                foreach ($p in $patterns) {
                    $c = ([regex]::Matches($line, $p, 'IgnoreCase')).Count
                    if ($c -gt 0) {
                        $key = "$rel|$p"
                        if ($hits.ContainsKey($key)) { $hits[$key] += $c } else { $hits[$key] = $c }
                    }
                }
            }
        }
    }
}

$current = @{}
foreach ($k in $hits.Keys) { $current[$k] = $hits[$k] }

# 生成/更新基线（按字典序写，无 BOM；行尾固定 CRLF 避免 diff 噪声）
if ($UpdateBaseline) {
    $out = @($current.Keys | Sort-Object | ForEach-Object { "{0}|{1}" -f $_, $current[$_] })
    [System.IO.File]::WriteAllText(
        $baselinePath,
        (($out -join "`r`n") + "`r`n"),
        (New-Object System.Text.UTF8Encoding($false))
    )
    Write-Host ("✓ 已写基线：{0} 条（扫描 {1} 个文件）→ tools/base-hygiene-baseline.txt" -f $out.Count, $fileCount)
    exit 0
}

if (-not (Test-Path -LiteralPath $baselinePath)) {
    Write-Host "✗ 找不到基线文件 tools/base-hygiene-baseline.txt"
    Write-Host "  本地先跑一次：pwsh tools/check-base-hygiene.ps1 -UpdateBaseline（人工核对后提交）"
    exit 1
}

$baseline = @{}
foreach ($l in [System.IO.File]::ReadAllLines($baselinePath)) {
    $t = $l.Trim()
    if (-not $t -or $t.StartsWith('#')) { continue }
    $parts = $t -split '\|'
    if ($parts.Count -lt 3) { continue }
    $k = $parts[0] + '|' + $parts[1]
    # 同一路径+模式写了多行时取累加（容忍手工编辑时的无意重复）
    $n = 0
    [void][int]::TryParse($parts[2], [ref]$n)
    if ($baseline.ContainsKey($k)) { $baseline[$k] += $n } else { $baseline[$k] = $n }
}

$violations = @()
foreach ($k in ($current.Keys | Sort-Object)) {
    if (-not $baseline.ContainsKey($k)) {
        $violations += ("{0}  → {1} 处（基线未登记）" -f $k, $current[$k])
    } elseif ($current[$k] -gt $baseline[$k]) {
        $violations += ("{0}  → 当前 {1} 处 > 基线 {2} 处（登记过的残留只许变少）" -f $k, $current[$k], $baseline[$k])
    }
}

if ($violations.Count -gt 0) {
    Write-Host "✗ 基座护栏：本体里出现了引擎身份（见 AGENTS.md 基座合同）："
    $violations | ForEach-Object { Write-Host "    $_" }
    Write-Host "  正确做法：把这段判断搬进插件（manifest / voices.label / ABI 自述）；"
    Write-Host "  确属本次无法消除的历史遗留，一并用 -UpdateBaseline 重写基线，并在 commit message 里说明原因。"
    exit 1
}

# 通过：提醒可以收紧的条目（不阻断，避免“改完还得跑一次刷基线”的死循环）
$loosened = @()
foreach ($k in ($baseline.Keys | Sort-Object)) {
    if (-not $current.ContainsKey($k)) {
        $loosened += "$k（已消失，基线可从 {0} 降为 0）" -f $baseline[$k]
    } elseif ($current[$k] -lt $baseline[$k]) {
        $loosened += "{0}（{1} → {2}）" -f $k, $baseline[$k], $current[$k]
    }
}
if ($loosened.Count -gt 0) {
    Write-Host "· 残留已减少，建议收紧基线：pwsh tools/check-base-hygiene.ps1 -UpdateBaseline"
    $loosened | ForEach-Object { Write-Host "    $_" }
}
Write-Host ("✓ 基座检查通过（扫描 {0} 个文件，命中 {1} 条均在基线内）" -f $fileCount, $current.Count)
```

- [ ] **Step 2：给脚本补 BOM（含中文的 .ps1 无 BOM 会被 PS 5.1 按 GBK 解码报错）**

```powershell
$p = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\tools\check-base-hygiene.ps1"
$b = [System.IO.File]::ReadAllBytes($p)
if (-not ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)) {
    [System.IO.File]::WriteAllBytes($p, [byte[]](0xEF,0xBB,0xBF) + $b)
    "BOM 已补"
} else { "已有 BOM" }
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-bom.ps1
```
预期：`✓ BOM 检查通过：…`。

- [ ] **Step 3：生基线并逐条人工核对（本 Task 的真正检查点）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1 -UpdateBaseline
Get-Content .\tools\base-hygiene-baseline.txt
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1
```
预期：第二次跑输出 `✓ 基座检查通过`。
**基线内容必须逐条对照下表判定“应该消失 / 可以登记”**（错了就是 Task 6-11 漏删）：

| 基线里**不得出现**（出现 = 前序 Task 漏删，回去补） | 出处 |
|---|---|
| `src-tauri/src/tts/mimo.rs|*` | Task 6 已删文件 |
| `src-tauri/src/tts/mod.rs|*` | Task 6 已改成 2 行声明 |
| `src-tauri/src/commands/clone_voice.rs|*` | Task 8 已去引擎化 |
| `src-tauri/src/commands/tts.rs|mimo` 与 `…|voiceclone` | Task 6 已删 `generate_mimo` |
| `src/components/Settings/ApiKeyModal.tsx|*` | Task 11 已删文件 |
| `src/components/Voice/VoiceSynthPanel.tsx|mimo` | Task 11 已删 MiMo 卡 |
| `src/App.tsx|mimo`、`src/stores/settingsStore.ts|mimo` | Task 11 Step 4 已改 |
| `src/components/Chat/VoiceInputButton.tsx|mimo` 等 4 处 ASR 文案 | Task 11 Step 4③ 已改 |
| `src-tauri/src/commands/settings.rs|mimo` | Task 5 已删 env 同步块 |

| 允许登记（挂号的存量，下一批拆） | 理由 |
|---|---|
| `…\storage\types.rs|mimo`、`src\types\index.ts|mimo`、`src\stores\settingsStore.test.ts|mimo` | `mimo_api_key` 字段本版本保留（设计 §3.4-4 两步走） |
| `src-tauri/src/plugins/mod.rs|mimo`、`…\storage\settings.rs|mimo` | Task 7 的一次性迁移映射表（`tts_model`/旧键退役后归 0） |
| `src-tauri/src/commands/remote.rs|mimo`、`src\services\invoke.ts|mimo` | `mimo_invite_code` 只读兼容字段 |
| `*|moss`、`*|minimax`、`*|genie`、`*|hojo`、`*|edge-tts` | 设计 §3.6 残留清单里“下一批”的项（含 `commands/plugins.rs` 的 `GenieData` 路径判断 31 处） |

- [ ] **Step 4：挂进 CI**

`.github/workflows/consistency.yml`：把头部注释里的 `Runs two read-only PowerShell checks`
改为 `Runs three read-only PowerShell checks`，并在注释块补一行：

```yaml
#   - tools/check-base-hygiene.ps1   : no engine identity in host code (AGENTS.md base contract)  
```

在 `- name: Desktop version four-place consistency` 步骤**之前**插入第三步（同形状，零架构成本）：

```yaml
      - name: Base hygiene (engine identity must live in plugins only)
        shell: pwsh
        run: ./tools/check-base-hygiene.ps1
```

- [ ] **Step 5：`AGENTS.md` 新增「基座合同」小节**

在 `## 项目硬性约定` 之后追加（位置固定，便于后人阅读）：

```markdown
## 基座合同（引擎身份不得写进本体）

本体只做底座（类比 harness），能力一律插件级、取舍由用户定。硬约束：

1. 宿主（`src-tauri/src`）与前端（`src`）**禁止**出现具体引擎的身份分支：
   `if engine == "xxx"`、硬编码的下拉项/音色表、`CLONE_CAPABLE` 这类引擎名白名单集合。
2. 引擎差异只能以**能力位/自述数据**存在：`has_stream()`、manifest `config` 字段、
   `voices.label` 文案（给用户看的提醒写在插件里）、远程 `plugin_help_url` 覆盖表。
3. 宿主只保留底座：引擎抽象、播放管线、存储、快捷键/窗口、设备、遥控桥。
4. 新引擎接入 = 新插件 crate；本体代码应**零修改**。做不到就是基座缺口，记进设计文档而不是贴补丁。

执行手段：`pwsh tools/check-base-hygiene.ps1`（已挂 `.github/workflows/consistency.yml`）。
存量残留登记在 `tools/base-hygiene-baseline.txt`，**只许变少不许变多**；
拆干净一段就用 `-UpdateBaseline` 收紧基线（并在 commit message 里说明）。`plugins/` 不扫。
行内貁免注释：`// baseok: <理由>`（与日志护栏的 `// allowlog:` 同口径）。
```

- [ ] **Step 6：护栏自检（故意破一次，确认真能拦）**

临时在 `src-tauri/src/commands/tts.rs` 末尾加一行带引擎名的假判断：

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
Add-Content -Path .\src-tauri\src\commands\tts.rs -Encoding utf8BOM -Value '// demo: moss 引擎占位注释，验证护栏会报错'
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1
```
预期：`✗ 基座护栏：… src-tauri/src/commands/tts.rs|moss → 当前 16 处 > 基线 15 处（登记过的残留只许变少）`，`exit 1`。
然后**删掉那行**（`git checkout -- src-tauri/src/commands/tts.rs` 属于红线操作，
需用户同意；用户不在场时手工删那行即可），再跑一次预期 `✓ 基座检查通过`。

- [ ] **Step 7：提交**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git add tools/check-base-hygiene.ps1 tools/base-hygiene-baseline.txt .github/workflows/consistency.yml AGENTS.md
git commit -m "chore(基座): 新增引擎身份护栏脚本 + 挂 consistency CI + AGENTS.md 基座合同

- check-base-hygiene.ps1 扫 src-tauri/src 与 src，命中引擎字面量即红；
  存量残留登记进基线（只许变少不许变多），-UpdateBaseline 收紧
- // baseok: 行内貁免，与 // allowlog: 同口径；plugins/ 不扫（插件写自家协议是本分）
- AGENTS.md 新增基座合同 4 条硬约束

Co-Authored-By: Claude <noreply@anthropic.com>"
```

## Task 13：全量门禁 + 实网探针 + 真机验收 + 文档回写

**Files:**
- Modify（不入库，`doc/` 在 git 仓库外）: `d:\Docs\Project\TTKook\doc\MiMo插件化与流式默认设计.md`、`d:\Docs\Project\TTKook\doc\工作线状态.md`
- Modify（入库）: `docs/superpowers/plans/2026-10-09-mimo-plugin-streaming-default.md`（本文件勾选框逐个打 `[x]`）

- [ ] **Step 1：全量门禁（七条，逐条贴输出到交付报告）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target"
cargo test --lib
cargo check --lib --tests
```
预期：**`173 passed; 0 failed; 3 ignored`** 且 `cargo check` **零 warning**。
算式（对不上就是某处漏删/漏测，回各 Task 的期望值逐段定位）：

| Task | 变化 | 累计 passed | ignored |
|---|---|---|---|
| 基线 main@dc55a57 | — | 170 | 4 |
| Task 5 | 不增不减（只换注入） | 170 | 4 |
| Task 6 | −8（mimo.rs）−2（tts/mod.rs）+1（空串归一） | 161 | 3 |
| Task 7 | +3（plugins/mod.rs 净增）+2（settings.rs） | 166 | 3 |
| Task 8 | +4（兜底选择纯函数 3 + 样本 env 契约 1） | 170 | 3 |
| Task 9 | 不加测试（线程接线） | 170 | 3 |
| Task 10 | +3（help_url 覆盖） | **173** | 3 |

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
cargo test -p mimo-tts
cargo test
```
预期：`mimo-tts` **23 passed; 0 failed; 2 ignored**（Task 1 的 16 例 + Task 2 的 5 例
+ Task 4 的 2 例；2 个 ignored 是实网探针）；全 workspace 无其他包 failed。

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
npx tsc --noEmit
npm run test:run
npm run build
```
预期：`tsc` 静默；`vitest` 全绿；`vite build` 成功。

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-bom.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-log-hygiene.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\scripts\verify-index.ps1 -SkipDist
```
预期：三道护栏全 `✓`，`verify-index` 输出 `ALL CONSISTENT`
（它**双向**校索引↔resources zip 且比对 zip 的 sha256，Task 3/5 的两个新 zip 必须都已 `git add`）。

- [ ] **Step 2：实网探针复跑（把 §四 的事实点钉死）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_TARGET_DIR = "d:\Docs\Project\TTKook\TTKook-mimo-plugin\plugins\target"
pwsh -NoProfile -ExecutionPolicy Bypass -File .\plugins\scripts\test-mimo-stream.ps1 -Clone
```
预期与记录项（写进 Step 5 的“实施情况”节）：

- 预置音色：块数 > 1，首块 TTFB 0.7–1.2s（存档分支实测值）；
- 克隆音色：恒 1 块、首块≈全部推完（实测 3.5s 级）；
- **克隆首块前 4 字节不得为 `RIFF`**；插件无条件实现了 `strip_wav_container`（首块命中才剔），
  探针打印剔前/剔后的首 4 字节以便事后判断；
- 落盘的 wav 能正常试听（开头无爆音、无双头）。
探针失败不等于门禁失败（需真 Key 与额度）；但**预置/克隆两条都必须跑通才能进 Step 3**，
否则不得声称验收完成（如实告知用户卡在哪一步）。

- [ ] **Step 3：真机人工验收（先测“老用户升级”，再测“全新安装”，顺序不能反）**

> 两个工作树共用 `%APPDATA%\com.voiceassist.app`，**不得删除**其中任何文件。
> 下面统一用“改名 → 测完改回”的可退做法。调试版与发布版互斥单实例，
> 跑前先 `tasklist | findstr /i voiceassist`。

**A. 老用户升级路径（最重要，弄错就是丢用户配置）**

1. 确认本机现在的 `settings.json` 里 `tts_engine` 是 `"mimo"`、`mimo_api_key` 非空：
   ```powershell
   Get-Content "$env:APPDATA\com.voiceassist.app\settings.json" | ConvertFrom-Json | Select-Object tts_engine, tts_model, clone_voice_path | Format-List
   ```
2. 跑本工作树（`npm run tauri dev` 或打包后的 exe），看 `app.log`：
   - 有 `插件已加载: mimo-tts`（bootstrap 自动装了内置包）；
   - **无** `MIMO_API_KEY` 相关的旧注入行。
3. 重读 `settings.json`，必须看到：`tts_engine: "mimo-tts"`、
   `plugin_voices["mimo-tts"]` 等于旧 `tts_model`（旧值为空/`default` 则是 `mimo_default`）、
   `plugin_config["mimo-tts"].api_key` 与 `plugin_config["mimo-asr"].api_key` 都已填上旧 Key、
   `mimo_api_key` 已清空、`tts_model` 已删除。
4. 语音中心：引擎下拉选中 “MiMo TTS（小米·流式）”，**API Key 卡里已经是“已设置”状态**
   （不需重填）；发一句长句→ **先出声后结束**；音色下拉里“克隆：我的音色（暂不支持低延迟）”
   存在，选中后能出声（首响慢属预期）。
5. ASR 回归：语音识别仍可用（Task 5 的硬性配对是否真生效）。

**B. 全新安装路径**

1. 备份并改名（不删）：
   ```powershell
   $d = "$env:APPDATA\com.voiceassist.app"
   Rename-Item -LiteralPath "$d\settings.json" "settings.json.bak-mimo-plugin"
   ```
2. 启动应用→ 预期默认引擎显示 **Edge TTS**，**不填任何 Key 打字即出声**。
3. 语音中心：能看到“MiMo TTS（小米·流式）”已装（免安装动作）、选它后要求填 Key。
4. 测完恢复：`Rename-Item -LiteralPath "$d\settings.json.bak-mimo-plugin" "settings.json"`
   并重启应用。

**C. 卸载与恢复**

1. 插件管理页卸载 MiMo → 当前引擎自动切到可用引擎（预期文案含“已自动切换到…」），
   且**不自动重装**（重启一次再看）。
2. 商店/插件库点“安装”恢复 → Key 不用重填（`plugin_config` 有意保留）、样本还在。
3. 卸载 Edge TTS（若当前引擎是它）→ 同样自动切到 MiMo（验证兜底不写引擎身份）。

**D. 本次新增的观感修复与代价（必须当场看）**

- 悬浮球：流式播放时**随播报波纹**（Task 9 补的 `va:play:start`）。
- 手机遥控：发字后遥控端显示“正在播放”（`playback:started{路径}` 已带字符串载荷），
  且遥控发字也是“先出声后结束”。
- 已知代价（属预期，写进发布说明）：
  1. 克隆音色首响仍是全文合成完成（实测 3.5s 级），官方能力缺口；
  2. **克隆的倍速（`playback_rate`）失效**：今天克隆走内置阻塞（倍速生效），
     插件化后克隆也走宿主流式管线，而倍速只作用于前端整段播放；
  3. Edge TTS 做默认引擎：微软非官方接口，可能不稳定/地区 403，失败时报中文错并提示可切引擎。

- [ ] **Step 4：回写设计文档（三处收紧 + 一节实施情况）**

编辑 `d:\Docs\Project\TTKook\doc\MiMo插件化与流式默认设计.md`（**该目录不在 git 仓库内，不需 `git add`**）：

① L3 状态行：

```markdown
> 状态：**已实施**（2026-10-09 v3 批准后按 `TTKook-mimo-plugin/docs/superpowers/plans/2026-10-09-mimo-plugin-streaming-default.md` 落地，13 个 Task；
> 实施中对本文件的三处收紧见 §九“实施情况”）。
```

② §四 末条（“一处必须实测”）补上结论与做法差异：写明
**实施时按“无条件实现 `strip_wav_container`”而非“探针判定后再写剥头分支”**
（把全设计唯一悬而未决的事实点用构造性防御关掉，探针退化为“观测 + 断言拿到可用 PCM”），
并贴上 Step 2 的实测输出。

③ §3.2-1 补一行实施口径：**播放态事件仅在 `spk_ok` 为真时发**（扬声器没开成时
由前端整段播并自行发事），而非“任一路开成”。

④ 文末新增一节：

```markdown
## 九、实施情况（2026-10-09，feat/mimo-plugin）

| 项 | 结果 |
|---|---|
| 插件 crate `plugins/engines/mimo-tts` | 已建（client/stream 分层，双导出）；单测 23 passed / 2 ignored |
| 宿主门禁 | `cargo test --lib` 173 passed / 0 failed / 3 ignored（基线 170：内置 MiMo 的 8 例搬进插件，新增 11 例迁移/兜底/覆盖测试） |
| 克隆首块 RIFF 实测 | （填 Step 2 实测值） |
| 预置音色块数 / 首块 TTFB | （填 Step 2 实测值） |
| 听感保护是否需补（预缓冲/水位） | （按 Step 2 结论填：实测无断音则不补） |
| 基座护栏基线条数 | （填 Task 12 实数，只许变少） |
| 遗留待下一批 | MOSS 内置、`minimax_clone`/`MinimaxVoicePanel`、`CLONE_CAPABLE`、`mimo_api_key` 字段删除、`mimo_invite_code` 只读字段 |
```

- [ ] **Step 5：更新工作线状态板**

在 `d:\Docs\Project\TTKook\doc\工作线状态.md` 增/改本工作线条目：分支 `feat/mimo-plugin`、
工作树 `TTKook-mimo-plugin`、基线 `main@dc55a57`、状态（已验收/待合并）、
关键结论（默认流式无降级、克隆靠服务端退化、基座合同已上护栏）与
“下一批拆什么”（§九 末行）。该文件同样在 git 仓库外，不需 `git add`。

- [ ] **Step 6：收口提交（勾选本计划 + 漏网文件）**

```powershell
cd d:\Docs\Project\TTKook\TTKook-mimo-plugin
git status --short
# 逐个 add（禁止 git add -A / git add .）：
git add docs/superpowers/plans/2026-10-09-mimo-plugin-streaming-default.md
git commit -m "docs(计划): MiMo 插件化与流式默认实施计划归档（含门禁与验收记录）

Co-Authored-By: Claude <noreply@anthropic.com>"
```

> `git status --short` 里若出现**不属于本任务**的未跟踪/已改文件（别的并行工作线），
> **停下告知用户**，不处理不提交。main 工作树（`VoiceAssist/`）已知有 3 个他人
> 未跟踪文件，本任务全程不应碰它们。

- [ ] **Step 7：收尾方式交给用户定**

全部门禁绿 + 人工验收过之后，按 `finishing-a-development-branch` 的惯例给用户三选一：
合回 `main` / 开 PR / 先挂在 `feat/mimo-plugin` 上随下次发版。
**不要自行 push、也不自行 merge**（涉及红线 git 操作需用户明确同意）。

---

## 附：已知风险与回退（执行中随时对照）

| 风险 | 拦网 |
|---|---|
| Task 5 拆成两次提交 → ASR 直接失 Key | Task 5 Step 7 硬性同一 `git add` 一次 commit；commit 前 `git status --short` 核对 6 个文件 |
| 新旧 zip 未同时入 git → `verify-index` Part A 双向校验失败 | Task 3/5/13 都显式跑 `verify-index.ps1 -SkipDist`；`.zip` 是跟踪文件，逐个 `git add`（含删除记录） |
| `cargo` 命令未设 `CARGO_TARGET_DIR` → 覆盖其他工作树的 exe（已发生过事故） | 每条 cargo 命令前显式设本工作树 target（src-tauri 与 plugins 两套，不可互换） |
| 删代码时漏删 import/局部变量 | `noUnusedLocals` 使 `tsc` 直接红（Task 11 Step 6）；Rust 侧以 `cargo check --lib --tests` 零 warning 为准 |
| 改结构体/默认值时忘改构造点与测试 | 三个已定位陷阱：`remote.rs:311` 的 `RemoteConfig` 字面量、`settings.rs:255`/`types.rs:377` 的默认值断言、`plugins/mod.rs:154/161` 的迁移测试 |
| 克隆样本 env 只在导入/删除时更新，启动不注入 | Task 5 已把 `VA_CLONE_SAMPLE` 启动注入接到 `lib.rs`，Task 8 又把两处实现收敛到同一 helper |
| 内置 MiMo 卸载后用户仍选它 | Task 8 的 `pick_fallback_engine` 只按能力位选，不写引擎身份（基座合同第 1 条） |
| 护栏一上就红一大片 | 基线机制 + Task 12 Step 3 的“应消失/可登记”对照表；登记项与 §九 末行同步挂号 |

