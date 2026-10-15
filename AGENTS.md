# AGENTS.md — AI 协作须知

> 任何 AI 工具（QoderWork / Claude Code / Codex 等）在本仓库工作前必读。
> VoiceAssist（电子声带）：Tauri v2 + React + TS + Rust 桌面 TTS 应用。

## 多工作线并行（重要）

本仓库可能同时有多个 AI 工作线在不同分支并行开发。
若有多线并行，工作线清单与注意事项见仓库外 `../doc/归档/工作线状态.md`（历史板，已全部合入 main 并发布；新开工作线时更新此板）。

1. **动手前**：`git branch -vv` + `git status --short`，确认 HEAD 和未提交
   改动都属于本次任务
2. 发现不属于本次任务的未提交改动：**停下询问人类**，不修改、不提交、不删除
3. 提交按文件名逐个 add，**禁止 `git add -A` / `git add .`**
4. `git reset --hard` / `git clean` / `git checkout --` 每次必须人类明确同意
5. 工作区有未提交改动时，切换/创建分支前先停下询问

## 项目硬性约定

- 回复、代码注释、commit message 用简体中文；commit **不追加 `Co-Authored-By` 后缀**
  （作者就是 git 记录的提交者本人；公开仓库历史不留第三方/AI 厂商邮箱。2026-10-10 由用户取消）
- 设计先行：每个开发阶段先写设计进 `../doc/开发记录.md`（阶段编号递增），
  用户确认后再编码，完成后回写完成情况
- cargo 构建/测试需 `CARGO_HTTP_CHECK_REVOKE=false`（本机证书吊销检查会失败）
- 要验证/发布 release 必须用 `npm run tauri build`，**不得拿 `cargo build --release` 代替**：
  后者出的 exe 不嵌前端资源（体积少约 0.19MB），后端能正常起但窗口白屏去连 devUrl
  `localhost:1420`；app.log 特征是**没有 `[perf] startup=` 行**（前端未挂载）。
  真机验证前端相关改动时先看这一行在不在，别等用户报界面异常
- 版本号三处同步：`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`package.json`
- PowerShell 5.1 陷阱：写 JSON 用 `UTF8Encoding($false)`（无 BOM）；
  含中文的 .ps1 脚本本身必须存为**带 BOM** 的 UTF-8；
  `$ErrorActionPreference = "Stop"` 下外部命令（gh 等）带重定向的 stderr
  会直接中断脚本，判断成败用 `$LASTEXITCODE`（临时降级 `"Continue"`）
- 插件开发规范：QoderWork 用户请加载 `voiceassist-dev` skill；
  其他工具读 `../doc/插件系统开发参考.md`（版本三处同步、索引与发布规则
  见其中「索引与发布规则」一节，发布用 `plugins/scripts/publish.ps1`）

## 基座合同（引擎身份不得写进本体）

本体只做底座（类比 harness），能力一律插件级、取舍由用户定。硬约束：

1. 宿主（`src-tauri/src`）与前端（`src`）**禁止**出现具体引擎的身份分支：
   `if engine == "xxx"`、硬编码的下拉项/音色表、`CLONE_CAPABLE` 这类引擎名白名单集合。
2. 引擎差异只能以**能力位/自述数据**存在：`has_stream()`、manifest `config` 字段、
   `voices.label` 文案（给用户看的提醒写在插件里）、远程 `plugin_help_url` 覆盖表。
3. 宿主只保留底座：引擎抽象、播放管线、存储、快捷键/窗口、设备、遥控桥。
4. 新引擎接入 = 新插件 crate；本体代码应**零修改**。做不到就是基座缺口，记进设计文档而不是贴补丁。

执行手段：`pwsh tools/check-base-hygiene.ps1`（已挂 `.github/workflows/consistency.yml`）。
存量残留登记在 `tools/base-hygiene-baseline.txt`（每行 `相对路径|模式|允许次数`），
**只许变少不许变多**：新增 (路径,模式)、计数变大、以及**变小/条目消失**都会红。最后一种
是在逼你当场收紧棘轮：若只拦“变大”，拆完一段后基线里留着白额度，下一个人可以在
同一文件重新写死同一个引擎而不被发现。收紧只需多跑一次 `-UpdateBaseline`（它只会把数字改小，
第二次必绿），不存在死循环。`plugins/` 不扫。
行内豁免注释：`// baseok: <理由>`，写在命中行的**本行或紧邻上一行**
（与日志护栏的 `// allowlog:` 同口径：块内/前一行/本行）。
接入新引擎时**先往脚本的 `$patterns` 加一条**，本体里所有写死它的地方当场变红；
端点名、遗留字段名这类变体也要单独列（已收：`voice_clone`、`edge_voice`、带引号的裸 `edge`），
否则拼写差一点就整段绕过。

### 在册豁免（读基线前先看这里，别照着上面的红线去拆）

基线里的 51 条都属存量，不是新写的违规。分三类：

> **「内置引擎」一类已清零**（2026-10-15 MOSS 插件化，`c8dac14`）：本体不再有内置
> TTS 引擎，`tts/moss.rs`、`tts/mod.rs`、`commands/tts.rs`、`commands/diag.rs` 与前端
> `VoiceSynthPanel.tsx`/`types/index.ts`/`invoke.ts`/`settingsStore.ts` 的 moss 条目
> 全部整条消失，基线由 57 条收到 51 条。

| 类别 | 条目 | 处置 |
|---|---|---|
| 遗留字段与默认值 | `storage/types.rs`、`storage/settings.rs`、`settingsStore.test.ts` 的 `mimo`/`minimax`/`moss`/`edge-tts` | 设计 §3.4-4 明确“字段保留、两步走退役”，本版本不动。`moss_*` 三字段已退出前端 `Settings` 类型（后端仍留作迁移读取源，与 `minimax_api_key` 同口径），下一版本删 |
| 一次性迁移表与运营字段 | `plugins/mod.rs`（旧标识→插件 id 映射；含 moss→moss-tts 的 Key/选中音色/音色列表搬迁）、`commands/remote.rs`（`mimo_invite_code`） | 旧键退役/线上 JSON 改字段后归 0 |
| **已知的基座缺口（下一批要拆）** | `commands/plugins.rs` 的 `genie` 31 处（基座写死了离线资源包的 zip 结构）、`minimax_clone.rs` + `MinimaxVoicePanel.tsx` + `invoke.ts`（MiniMax 克隆通道整体还在宿主/前端）、`usePluginInventory.ts` 的 `CLONE_CAPABLE` 集合（**就是第 1 条的在册反例**）、`InstalledCard/PluginSetupPanel/ResourcePackLinks.tsx` 的 `p.id === "genie-tts"` 分支 | 能力应改由插件自述（资源清单进 manifest、克隆能力进能力位）；拆一段跑一次 `-UpdateBaseline` |

测试夹具里的引擎名（`perf.rs`、`manifest.rs`、`registry.rs`、`pluginTaskStore.test.ts` 等）
只是样例数据，不必拆；它们占位是为了“数字不变就是没新写入”。

## 存储契约（数据路径只能来自 `storage::paths`）

与基座合同并列的另一条硬约束。为什么需要它：全项目向系统要数据目录的代码只有一处，
但“数据放在哪”的知识曾散在 19 个模块的 `join("audio")` 里——配置与内容分根之后，
任何一处自己拼路径都会**静默地**把数据写错地方（而且两个根都是 `PathBuf`，编译器一个字不会报）。

1. 业务代码**禁止** `app_data_dir()` / `current_exe()` 取数据路径，也禁止自己
   `join("audio")` 之类；一律走 `storage::paths::StoragePaths` 的 getter，
   只持有 `&Path` 时用 `audio_dir_in` / `samples_dir_in` / `downloads_dir_in`。
2. **配置根**（%APPDATA%）只放 settings / 遥控配对 / 远程配置缓存 / 存储指针；
   **其余一律内容根**（跟随安装位置）。消息与收藏必须和 `audio/` 同根——
   删消息要按引用计数删音频，分家会把引用判定弄断。
3. 记录里不得写绝对路径（`audio_path`、`clone_voice_path` 保持相对内容根）。
4. `<插件根>` 只放代码（dll/manifest/registry）；模型与运行环境在
   `<内容根>\plugin-data\<id>`，**卸载插件默认不得牵连它**（默认保留数据，界面必须二选一）。
5. 执行手段：`pwsh tools/check-storage-hygiene.ps1`（已挂 consistency CI）。基线
   `tools/storage-hygiene-baseline.txt` 每行 `路径|模式|次数`，**只许变少**（变大/变小/消失全红）；
   行内豁免 `// storageok: <理由>` 只认**本行或紧邻上一行**——多行说明时标记必须写在调用正上方，
   写在第一行会静默失效（实测踩过）。`storage/paths.rs` 不在扫描范围：它就是路径的唯一主人，
   扫它只会刷掉“别处出现即红”的信号。
