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

- 回复、代码注释、commit message 用简体中文；commit 结尾带
  `Co-Authored-By: Claude <noreply@anthropic.com>`
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
存量残留登记在 `tools/base-hygiene-baseline.txt`，**只许变少不许变多**；
拆干净一段就用 `-UpdateBaseline` 收紧基线（并在 commit message 里说明）。`plugins/` 不扫。
行内豁免注释：`// baseok: <理由>`（与日志护栏的 `// allowlog:` 同口径）。
接入新引擎时**先往脚本的 `$patterns` 加一条**，本体里所有写死它的地方当场变红。
