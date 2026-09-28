# 千问 ASR 接入 + 字幕流式改造 · 设计

日期：2026-09-27 · 状态：设计已定稿，待用户复核 → 转实施计划

- **实施基线**：本地 `main`（`fbb0bfa`）新建 `feat/qwen-asr` 分支 + 独立 worktree。
  选它的证据：字幕链路文件（`audio_capture/`、`commands/subtitle.rs`、`asr/`）与
  `chore/plugins-cleanup` 分支**逐字节相同**（`git diff --stat` 为空）；main 另含 8 个
  未推送的已验收修复（asset scope/CSP、VB-CABLE SHA-256、插件配置并发、遥控重连）。
- **目标版本**：本体 **v1.9.0** / 新插件 qwen-asr 0.1.0。注意 `v1.8.8` 已被占用且属
  「仅存档未发布」（见 §10），**不可复用该号**。

## 1. 背景与目标

字幕功能目前只有 MiMo ASR 一个云端引擎，且字幕链路是「本地能量 VAD 切句 → 整句 WAV →
一次性 HTTP 转写」。对听障用户在 KOOK 语音里跟随对话而言，最疼的两条是**延迟**与**断句**：

- 延迟：`hangover_seconds = 0.75s` 定长静音判停 + 每句重新建连上传，实测「说完到看见字」约 1.6–3.5s，
  且这段时间屏幕完全空白。
- 断句：本地按静音硬切，喘口气就把一句话说成两条；`max_speech_seconds = 12s` 还会强切长句。

2026-09-23 阿里发布 Qwen-Audio-3.1 语音模型系列，其中
`qwen-audio-3.1-asr-flash-streaming`（WebSocket 实时）与 `qwen-audio-3.1-asr-flash`
（HTTP，支持方言转普通话与即时热词）可同时改善延迟、断句、准确率三项。

本设计**不含说话人分离**（用户决定暂缓，且官方明确该能力仅在非实时模型上）。

## 2. 决策

| 决策 | 内容 |
|---|---|
| 接入形态 | 新增独立 ASR 插件 `qwen-asr`，不替换 `mimo-asr` |
| **默认引擎** | **保持 mimo**（用户决定 #2）。qwen 作为可选增强，不改动任何用户已有默认值 |
| 字幕路径 | 走**流式**（`-streaming`），废掉本地能量 VAD 的切句职责 |
| 语音输入路径 | 走**一次性**（`asr-flash`），按住说话场景 WebSocket 无收益 |
| **润色开关** | **提供**（用户决定 #1），实现为模型代际选择，见 §7 |
| 能力发现 | 以**符号可否解析**为准，不加 manifest 能力位（见 §4） |
| 静默门控 | **实现在插件内部**，宿主只管连续 feed（见 §6） |

### 对已展示设计的两处自我修正

1. **删掉 `SubtitleLine.speaker: Option<u32>` 预留字段。** 原方案称「为将来分离预留，避免二次改格式」，
   但带 `#[serde(default)]` 的字段日后新增本就零成本，而一个恒为 `None` 的死字段会让读代码的人误判
   「系统里有说话人概念」，属实际危害。改为将来真做分离时再加。
2. **删掉 `manifest.asr_streaming` 能力位。** 宿主 `libloading` 能直接探测符号是否存在，
   与现有可选符号（`va_tts_plugin_setup!` 那套「取不到就当不支持」）惯用法一致；
   加字段只会造出「声明与实现可能不一致」的第二个真相源。
3. **静默门控从宿主挪进插件。** 原方案在宿主 vad.rs 新建 `ActivityGate`，但插件本就只收到 PCM，
   自己算 RMS 是 20 行代码；挪过去后 ABI 不需要控制通道（少一个 `stream_control` 符号），
   宿主会话代码也更薄。代价是 RMS 计算与宿主的分段路径各存一份，可接受。

## 3. 已核实事实（全部来自阿里云百炼官方文档正文）

| # | 事实 | 来源 | 置信 |
|---|---|---|---|
| F1 | 说话人分离**仅非实时**模型支持；streaming 明确「不支持」 | 语音识别选型页模型表 | 高 |
| F2 | `qwen-audio-3.1-asr-flash` 支持 Base64 Data URI、≤5 分钟、句+字级时间戳 | 同上 + HTTP API 页 | 高 |
| F3 | 流式协议 = DashScope WS：`run-task`→`task-started`→二进制音频→`result-generated`→`finish-task`→`task-finished`；音频须**单声道**；100ms/3200 字节为一个 chunk | WebSocket API 页 + 示例码 | 高 |
| F4 | 一次性接口 `X-DashScope-SSE: enable` 可流式返回，但**要求音频 ≥1 分钟**，逐句短片段用不上 | HTTP API 页请求头表 | 高 |
| F5 | `keep_dialect` boolean（可选）**仅 `qwen-audio-3.1-asr-flash`（非实时）支持**；默认 false = 将方言转写为普通话文本，true = 保留方言 | HTTP API 页 parameters 表原文 | 高 |
| F6 | 即时热词 `vocabulary`：`{"张三":5}` 键值对，权重 `[1,5]` 或 `50`（超级热词，≤50 个）；仅 3.1/3.0-asr-flash 支持；与预编译热词合并后 >2000 条随机取 2000 | HTTP API 页 parameters 表原文 | 高 |
| F7 | 润色：官方原文「Qwen-Audio-3.0-ASR-Flash 和 Fun-ASR-Flash 的润色顺滑功能**默认关闭，暂未开放**。Qwen-Audio-3.1-ASR-Flash 支持原生文本润色」；**未给出 3.1 的润色关闭参数** | HTTP API 页 parameters 说明段原文 | 高 |
| F8 | **流式文档全文无「润色」字样**（已 grep 验证） | 实时语音识别用户指南 | 中（覆盖面受限于抓取正文） |
| F9 | 计费转 Token：音频 25 Token/秒；streaming 输入 6 元/百万、asr-flash 输入 0.8 元/百万 | 模型定价页「千问Audio」节 | 中（换算率需账单复核） |
| F10 | **接口域名必须带 Workspace ID**，官方 HTTP/WS 全部示例一律是 `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/...`、`wss://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/api-ws/v1/inference`；正文注明「请将 {WorkspaceId} 替换为真实的业务空间 ID，各地域配置不同」 | 两个 API 页的 URL 小节 + 全部示例代码 | 高 |
| F11 | 3.1-streaming 的断句旋钮**只有** `vad_model`：`near_meeting_16k`（近场）/ `far_field_meeting_16k`（远场，**默认**）；`max_sentence_silence` 是 3.0-streaming/Fun-ASR/Paraformer 的参数，3.1 用不了 | 实时指南「VAD」小节原文 | 高 |
| F12 | 流式返回结构 `payload.output.sentence.{begin_time,end_time,text,sentence_end,words[]}`；`sentence_end=false` 时 `end_time` **可能为 null**，文本与时间戳仍会变 | WS 页时间戳小节 + 示例响应 | 高 |
| F13 | 选型页把 3.1-asr-flash-streaming 明确列为「实时字幕」推荐模型；能力列有热词、Prompt 上下文、多语种及方言 | 语音识别选型页模型表 | 高 |

F5 同时划出一条**未证实项**：`keep_dialect` 与 `vocabulary` 都只在**非实时** asr-flash 的参数表里定义，
流式侧仅写「支持热词，配置方法见另一页」，未给出 `run-task` 内的字段路径。故 §6 明确区分两条路径的参数集，
不假装流式也能转普通话。

F7 + F8 合起来的推论：**润色是 asr-flash（语音输入路径）的问题，字幕路径大概率不涉及**。
§7 的开关按此设计，并用 §9 集成测试的润色探针证伪。

## 4. ABI 扩展（`plugins/plugin-api/src/lib.rs`）

新增 5 个**可选导出**符号与类型别名：

```rust
pub const SYM_ASR_STREAM_START:  &[u8] = b"va_asr_stream_start\0";
pub const SYM_ASR_STREAM_FEED:   &[u8] = b"va_asr_stream_feed\0";
pub const SYM_ASR_STREAM_POLL:   &[u8] = b"va_asr_stream_poll\0";
pub const SYM_ASR_STREAM_FINISH: &[u8] = b"va_asr_stream_finish\0";
pub const SYM_ASR_STREAM_CLOSE:  &[u8] = b"va_asr_stream_close\0";

// 建立流式会话。config_json：{"language":"zh","sample_rate":16000}
unsafe extern "C" fn(config_json: *const c_char, out_handle: *mut *mut c_void,
                     out_err: *mut *mut c_char) -> i32;
// 送一段 16k/16bit/mono PCM。仅入有界队列，不得阻塞、不得发网络。
unsafe extern "C" fn(handle: *mut c_void, pcm: *const u8, len: usize) -> i32;
// 取增量结果。有结果写 out_json（CString，宿主 va_free_cstr 归还）返回 VA_OK；
// 无结果写 "[]"；出错返回 VA_ERR 并写 out_err。
unsafe extern "C" fn(handle: *mut c_void, out_json: *mut *mut c_char,
                     out_err: *mut *mut c_char) -> i32;
// 通知结束（finish-task），冲刷剩余句子，不关闭连接
unsafe extern "C" fn(handle: *mut c_void, out_err: *mut *mut c_char) -> i32;
// 关闭并释放 handle（幂等，永不上抛 panic）
unsafe extern "C" fn(handle: *mut c_void);
```

`poll` 的 JSON schema（宿主唯一需要理解的形状，厂商协议不外泄）：

```json
[{"text":"队友：中路miss了","begin_ms":120,"end_ms":1400,"stable":true},
 {"text":"他那边","begin_ms":2100,"end_ms":null,"stable":false}]
```

`stable = true` 当且仅当该句已 `sentence_end`；false 表示还会被后续事件修正（F12）。
`end_ms` 在中间态可为 `null`，故它是 `Option<u64>` 而非 `u64`。

> **`begin_ms` 的时间原点**：它是**本次 task 音频流**的相对毫秒，`finish-task`/重连后归零，
> 因此只能在「同一 handle 存活期内」用作句子身份键，**不可**当全局时钟；
> 落盘与显示一律用宿主的 wall-clock `ts`（沿用现有 `now_ms()`）。

约束（写进文档注释）：

- **不动 `VaHostServices`**。本次只新增插件导出符号，不往宿主能力桥里加字段。
  原因：`audio_effect` 那条线已在改 `plugin-api` 里的
  `assert_eq!(size_of::<VaHostServices>(), (2 + 10) * size_of::<usize>())`；两边都改这个结构体
  就会造成语义冲突（不只是文本冲突）。已核实那条线没动 ASR 相关符号
  （`git diff main...chore/plugins-cleanup` 中 `va_asr`/`ASR_` 新增行 = 0），保持互不侵犯。
- **同一 handle 的 feed/poll/finish/close 由宿主在同一线程串行调用**（会话线程），
  插件据此不必为 handle 加锁。
- 全部函数体 `catch_unwind(AssertUnwindSafe(..))` 包裹，panic 转 VA_ERR + 中文错误串；
  `close` 吞掉 panic。
- 新增独立宏 `va_asr_stream_plugin!`，只生成这 5 个符号（不与 `va_asr_plugin!` 的
  `va_asr_languages`/`va_asr_transcribe` 重名冲突）。插件同时调用两个宏即得双能力。

`LoadedAsrPlugin`（宿主侧）新增 `stream_start/feed/poll/finish/close` 方法，
`supports_stream() -> bool` 由「上述**五个**符号是否全部取到」计算；缺任意一个即视为不支持流式，
整组按原子能力对待，不做部分启用（避免出现「能 start 不能 close」这种必然泄漏 handle 的组合）。

## 5. 宿主字幕链路改造（`audio_capture/session.rs`）

`spawn_session` 按插件能力二选一，**分段路径原样保留**（mimo 等继续走它）：

```
流式路径：capture → f32→i16 PCM 字节 → plugin.feed()
                                   ↘ plugin.poll() 每循环一次 → stable 判定 → emit
分段路径：capture → EnergyVad → WAV → transcribe → emit   （不动）
```

- 循环节拍沿用现有 `rx.recv_timeout(50ms)`，即每 ≤50ms feed + poll 一次。
- **暂停（Alt+M）**：`finish` → `close` → 丢弃 handle；恢复时重新 `start`。
  与分段路径「丢弃样本」行为一致，且不产生暂停期间的音频外流。
- **停止监听**：`finish` → 有界轮询 `poll` 直到无新结果或超 2s → `close` → 收编历史。
- **落盘规则**：`subtitle_history.json` **只存 stable 句子**，中间态不进历史与导出，
  因此回看/导出格式与 v1.8.7 完全兼容（旧文件无需迁移）。
- 事件 `asr:subtitle` payload 扩为 `{text, ts, stable}`；
  `SubtitleWindow.tsx` / `SubtitlePage.tsx`：`stable=false` 时**就地覆盖**当前临时行，
  `stable=true` 时落定为新行。前端只做渲染策略改动，不引入新依赖。
- **同句识别规则**：宿主以 `begin_ms` 为 key 维护「在途句」小表（同 handle 内唯一）。
  再次 poll 到相同 `begin_ms` 且 `stable=false` → 覆盖该 key 的文本；`stable=true` → 从在途表
  移除并按 `ts` 追加为正式行。key 不同且仍在途的条目最多保留 3 条（超过说明服务端异常，
  丢弃最旧并记一次中文告警），避免临时区无限增长。
- **重连/挂起恢复后** `begin_ms` 归零：宿主在 `finish`/`close` 时**清空在途表**，
  防止新 task 的 `begin_ms=170` 误命中上一段的同值 key。
- 错误处理：`feed`/`poll` 返回 VA_ERR → 停止会话、emit 中文错误、保留已产出字幕。
  **不做「自动回落到分段路径」的静默降级**（会让用户以为引擎坏了却没提示）。

## 6. 插件 `qwen-asr`（`plugins/qwen-asr/`）

依赖 `reqwest`(rustls) + `tokio` + `tokio-tungstenite` + `serde_json` + `base64`。
自带 tokio 运行时（沿用 mimo-asr 的 `OnceLock<Runtime>` + `block_on` 惯用法）。

- **一次性**（`transcribe`）：`POST https://{host}/api/v1/services/aigc/multimodal-generation/generation`，
  头 `Authorization: Bearer <key>`、`Content-Type: application/json`、`X-DashScope-SSE: disable`；
  body `input.messages[type=input_audio].input_audio.data = "data:audio/wav;base64,..."`，
  `parameters = {format:"wav", sample_rate:"16000", keep_dialect, vocabulary}`（注意这里的
  `sample_rate` 官方示例是**字符串**）。取 `output.text`；`usage.duration` 供计费展示。
- **流式**：`WSS wss://{host}/api-ws/v1/inference`，头 `Authorization: bearer <key>`，按 F3 时序。
  `run-task` 的完整形状（官方示例逐字对上）：`header.{action:"run-task", task_id:<32位 hex>,
  streaming:"duplex"}` + `payload.{task_group:"audio", task:"asr", function:"recognition",
  model, parameters, input:{}}`，其中 `parameters = {sample_rate:16000, format:"pcm", vad_model}`
  ——**这里的 `sample_rate` 是数字**（与一次性路径不同形，别写错）；宿主给的是裸 PCM 无 WAV 头，
  故 `format` 必为 `pcm`。音频按 100ms/3200 字节分块（F3）。`finish-task` 同 header 形状、
  `action:"finish-task"`。解析 `result-generated` → `payload.output.sentence{.begin_time,.end_time,
  .text,.sentence_end}`（F12）。`vad_model` 默认取 `near_meeting_16k`（耳机近场人声；官方的
  `far_field_meeting_16k` 默认值面向会议室远场，不适配我们的场景），可由配置覆盖（见下表）。
- **流式路径的参数集与一次性不同**：`keep_dialect`/`vocabulary` 只在非实时参数表里有定义（F5/F6
  + 未证实项），因此 M1 **不往 `run-task` 里塞 `keep_dialect`**；热词先只作用于一次性路径，
  流式侧等「提升识别准确率」页的 `run-task` 字段路径核实后再补。
- **静默门控（插件内）**：滚动 RMS，连续静音超过 `VA_QWEN_IDLE_HOLD` 即 `finish-task` 挂起；
  恢复有声时先用保留的 300ms 预缓冲补发再 `run-task`。副作用：挂起窗口起点可能丢字。
- **有界队列**：feed 入队上限 5s 音频（160 KB），满则丢最旧并计数（防插件被网络卡死后宿主 OOM）。
- **重连**：WS 断开指数退避重连，最多 3 次；握手期 401/403 直接判定为 Key/地域问题不重连。
- 错误文案中文化，且**不回显响应体原文**（可能含用户语音内容）——沿用 mimo-asr
  「只报状态码与字节数」的做法。

### 配置项（manifest `config.fields`，UI 由 `PluginConfigPanel` 自动渲染，前端零改动）

| key | type | env | 说明 |
|---|---|---|---|
| `api_key` | secret | `DASHSCOPE_API_KEY` | 必填；`help_url` 指向百炼控制台 Key 管理页 |
| `workspace_id` | text | `VA_QWEN_WORKSPACE_ID` | **必填**（F10）；百炼控制台右上角「业务空间」里可复制 ID |
| `region` | select | `VA_QWEN_REGION` | `beijing`(默认) / `singapore`；**两地 Key 不通用** |
| `transcribe_style` | select | `VA_QWEN_STYLE` | `polish`(顺句·默认) / `literal`(原话) → §7 |
| `keep_dialect` | select | `VA_QWEN_KEEP_DIALECT` | `off`(转普通话·默认) / `on`(保留方言)；仅影响一次性路径 |
| `hotwords` | text | `VA_QWEN_HOTWORDS` | 逗号分隔，可写 `词:5`；超级热词 `词:50` 最多 50 个（系统硬限）；单次输入条目我们自行截到 200（非系统限制，系统合并后上限 2000） |
| `stream_vad` | select | `VA_QWEN_STREAM_VAD` | `near`(近场·默认) / `far`(远场) → `vad_model`；3.1 无阈值可调（F11） |
| `idle_hold` | select | `VA_QWEN_IDLE_HOLD` | `3s` / `8s`(默认) / `15s` / `never` |

**域名构造（F10）**：`beijing` → `{workspace_id}.cn-beijing.maas.aliyuncs.com`，
`singapore` → `{workspace_id}.ap-southeast-1.maas.aliyuncs.com`；HTTP 与 WSS 共用同一个 host 变量。
`workspace_id` 为空时，`transcribe`/`stream_start` 直接返回中文错误「缺少业务空间 ID（Workspace ID），
请在插件配置里填写」，**不回退到 `dashscope.aliyuncs.com` 公共域名**——官方文档没有任何这种写法，
先赌它能用等于把用户推进一个难定位的 DNS/404 报错里。若后续实测证实公共域名可用，再单独放宽。

## 7. 润色开关（用户决定 #1 的具体落地）

F7 表明 3.1 的润色**没有文档化的关闭参数**。因此不能凭空造一个假的开关骗用户，
落地为**模型代际选择**——这是当前唯一真实可控的「润色 on/off」：

- `polish`（顺句·默认）→ 一次性路径用 `qwen-audio-3.1-asr-flash`（带润色，读起来通顺）
- `literal`（原话）→ 一次性路径用 `qwen-audio-3.0-asr-flash`（官方明写润色默认关闭、暂未开放）

已知代价，写进字段 description：选 `literal` 时 `keep_dialect` 失效（3.0 不支持，
且 3.0 的 CER/方言能力弱于 3.1）。

字幕（流式）路径：M1 固定用 3.1-streaming。若 §9 集成测试的润色探针证明 3.1-streaming 会润色，
则补 `stream_style` 同构开关（3.0-asr-flash-streaming 存在）；若证伪，则**不**为字幕显示该开关。
「宁可少一个开关，也不放一个无效开关」。

## 8. 成本

按 F9（25 Token/秒）估算，streaming 输入 6 元/百万 Token → **≈0.54 元/小时**监听全程。
`idle_hold = 8s` 的门控可把实际计费压到真实语音量的约 1.2–1.5 倍。
一次性路径 0.8 元/百万 → ≈0.07 元/小时音频，可忽略。
新用户有 90 天免费额度（百万 Token 级），M1 验证阶段预计不产生费用。

## 9. 测试计划

**单元测试（进 `cargo test --lib`）**

- T1 ABI JSON：`poll` 结果编解码、空结果 `"[]"`、`end_ms` 为 `null` 能解成 `None`、非法 JSON 不 panic
- T2 代际映射：`transcribe_style` × `keep_dialect` 组合出的 model 名与参数正确（含 `literal` 时强制忽略 keep_dialect）
- T3 热词解析：`词:50`/非法权重/超 200 截断/超级热词超 50 降级
- T4 门控：RMS 序列驱动的挂起/恢复状态机 + 300ms 预缓冲不丢句首
- T5 宿主路径选择：只导出一次性符号的插件走分段路径；两者都有走流式路径
- T6 暂停/停止时序：finish→close 幂等，历史只收 stable；close 后在途表为空
- T7 域名与参数形态：`workspace_id` + `region` → 期望 host（两地）；`workspace_id` 空 → 中文错误；
  一次性 body 的 `sample_rate` 为字符串、`run-task` 的为数字（防止写反后到真机才发现）

**集成（`#[ignore]`，需真实 Key）**

- 一次性：`曼波.mp3` → 非空中文文本
- 流式：喂同一文件的 PCM，断言至少收到一个 `stable=true` 且文本非空
- **润色探针**：构造含大量语气词/口吃的音频，分别跑 `polish` 与 `literal`，
  以及 3.1-streaming；输出三者差异供决策（验证 F7/F8 推论）

**真机**：KOOK 语音挂 10 分钟，肉眼对比 v1.8.7 的字幕延迟；断网 30s 再恢复验证重连与错误提示。

## 10. 发布

**版本号前提（核实于 2026-09-29）**：最新**已发布**版本仍是 **v1.8.7**。
`v1.8.8` 是一个附注 tag，指向 `fb35892`，tag 正文自己写「v1.8.8 存档快照（未发布）……
仅存档，未走 tauri build 发布流程，应用版本号仍为 1.8.7」——即 **1.8.8 这个号已被 tag 占用但从未发过包**。
因此本次不能拿 1.8.8 当目标版本（tag 名冲突），也不适合改叫 1.8.9（本次是 ABI 变更 + 新插件，
不是补丁级）→ 直接用 **v1.9.0**，并在新 Release 时保持 tag 名与包版本一致。

- 本体 **v1.9.0**（ABI 与字幕行为变更，必须升本体版本；插件独立分发不能覆盖本体改动）
- `qwen-asr-0.1.0.zip` 进 `plugins-index.json`，`min_app_version = "1.9.0"`，
  双通道分发复用现有 `gitee-release.ps1`
- 本体默认引擎、默认字幕设置**一律不改**（决策 #2）
- 开工前需确认的一件事：本地 `main` 比 `origin/main`/`gitee/main` 领先 8 个提交（未推送）。
  本次不依赖推送，但发 v1.9.0 前必须先补推送两端，否则 Release 基线与 tag 会对不上。

## 11. 明确不做

- 说话人分离 / `speaker_id` / 声纹（用户暂缓；官方能力也不在实时侧）
- 预编译热词列表（`vocabulary_id`）——即时热词已够，省掉一套 CRUD
- `language_hints`（不设，让模型自动检测；游戏语音中英混杂，预设 hint 反而可能压制英文黑话）
- `X-DashScope-WorkSpace` 请求头——域名已含 Workspace ID，不重复传
- ASR-Next（API 截至本文档尚未上线）
- 为 qwen 写专用 `ApiKeyModal`（走通用配置面板，mimo 的硬编码属历史遗留，不复制）
- `SubtitleLine` 的 speaker 预留字段

## 12. 风险

| 风险 | 处置 |
|---|---|
| 3.1-streaming 实际也有润色 → 字幕失真 | §9 集成测试的润色探针实测；成立则按 §7 补 `stream_style` |
| 流式侧热词字段路径未证实 | M1 热词只作用一次性路径；流式不注入，不做静默猜测 |
| 用户不知道去哪找 Workspace ID → 配置负担变重 | 字段 `description` + `help_url` 写明控制台路径；缺失时报错文案直接指路，不做默认回退 |
| 3.1 断句手感不对 → 字幕碎/慢 | 只有 `vad_model` 一个旋钮（F11），真机对比 near/far 后定默认值 |
| 闭源 API，模型 ID 与字段可能变 | 全部厂商细节封在插件里；宿主 ABI 不含厂商语义 |
| 常驻 WS 计费超预期 | `idle_hold` 默认 8s；`usage` 里的计费秒数透传至 `poll` 结果（display 字段，供后续版本展示） |
| 游戏混音重叠说话，识别质量未知 | 与 mimo 同录音 A/B 对比后再定宣传口径 |
| 弱网断流 | 指数退避 3 次 + 明确中文错误，绝不静默 |
