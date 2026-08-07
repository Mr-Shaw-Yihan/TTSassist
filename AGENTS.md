# AGENTS.md — AI 协作须知

> 任何 AI 工具（QoderWork / Claude Code / Codex 等）在本仓库工作前必读。
> VoiceAssist（电子声带）：Tauri v2 + React + TS + Rust 桌面 TTS 应用。

## 多工作线并行（重要）

本仓库可能同时有多个 AI 工作线在不同分支并行开发。
当前工作线清单与注意事项见 `../doc/工作线状态.md`（仓库外，务必读）。

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
- 版本号三处同步：`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`package.json`
- PowerShell 5.1 陷阱：写 JSON 用 `UTF8Encoding($false)`（无 BOM）；
  含中文的 .ps1 脚本本身必须存为**带 BOM** 的 UTF-8；
  `$ErrorActionPreference = "Stop"` 下外部命令（gh 等）带重定向的 stderr
  会直接中断脚本，判断成败用 `$LASTEXITCODE`（临时降级 `"Continue"`）
- 插件开发规范：QoderWork 用户请加载 `voiceassist-dev` skill；
  其他工具读 `../doc/插件系统开发参考.md`（版本三处同步、索引与发布规则
  见其中「索引与发布规则」一节，发布用 `plugins/publish.ps1`）
