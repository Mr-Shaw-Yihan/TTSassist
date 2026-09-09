# 应用内升级与更新说明渲染 — 设计

日期：2026-09-09 · 状态：已批准（用户选定方案 A）· 目标版本：v1.8.5

## 背景与问题

现状三处缺陷：

1. 更新弹窗把 Release 的 markdown 正文原样塞进 `whitespace-pre-wrap` 容器，用户看到的是 `## v1.8.4` 这类源码，观感差。
2. "前往下载"只做 `openUrl` 跳浏览器，用户被丢到网页；**Gitee 国内分发链路对本体完全没起作用**（实测：`raw/dist/*.exe` 403、Gitee 上本体从未建 Release 404、只有 GitHub 附件 200）。
3. 检查更新以 GitHub API 为主通道，国内常不可达时才靠 Gitee tags 兜底，而该兜底**拿不到更新说明**（notes 恒为空）。

目标：在软件内完成"发现新版本 → 选通道下载 → 校验 → 一键安装并重启"，国内首选 Gitee、次选 GitHub，两通道都失败时给出可复制群号与手动直链。

## 决策记录

| 议题 | 结论 |
|---|---|
| 架构方案 | A：单一清单 `app-version.json` + 双通道对称镜像（弃：B 隐式换 URL、C 官方 tauri-plugin-updater） |
| 安装时机 | 下载校验完成后**不自动动手**，由用户点「立即安装并重启」 |
| markdown 渲染 | 自写 ~80 行零依赖渲染器（不引 react-markdown，保持前端零 UI 依赖现状） |
| 重发方式 | 升 **v1.8.5** 发新版，不覆盖已发布的 v1.8.4（同版本号触发不了更新提示，且违反"已发布产物不可变"） |
| 群号来源 | 不进清单；前端共享常量，复用关于页既有的"点击复制 + 已复制提示"实现 |

刻意不做（YAGNI）：更新通道偏好设置项、增量/delta 升级、强制更新门槛、官方 updater 插件接入。

## 数据契约

`app-version.json`（UTF-8 无 BOM，每次本体发布生成）：

```json
{
  "version": "1.8.5",
  "file": "TTSassist_1.8.5_x64-setup.exe",
  "size": 20508684,
  "sha256": "<hex>",
  "notes": "<release 正文 markdown>",
  "gitee_url": "https://gitee.com/yihwan/TTSassist/releases/download/v1.8.5/TTSassist_1.8.5_x64-setup.exe",
  "github_url": "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.8.5/TTSassist_1.8.5_x64-setup.exe"
}
```

清单获取顺序（改后）：**Gitee raw 优先**（8s 超时）→ GitHub `releases/latest/download/app-version.json`（10s）→ 都失败返回 null（保持"静默不打扰"）。

存放位置三份，互为镜像：Gitee `dist` 分支 raw（国内主入口，实测 raw 对 JSON 返回 200）、GitHub 本体 Release 资产（`releases/latest/download/`）、origin 的 `dist` 分支（当前 dist 只存在于 Gitee，GitHub raw 实测 404，需补推）。

## 安全边界

因为最终要执行下载来的 exe，以下校验是硬要求，任一不满足即拒绝执行：

1. `gitee_url` / `github_url` 必须命中编译期常量域名前缀 `https://gitee.com/yihwan/TTSassist/releases/download/` 与 `https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/`。
2. `file` 必须形如 `TTSassist_<version>_x64-setup.exe`，且内嵌版本等于 `version`。
3. 落地文件 SHA-256 必须等于清单 `sha256`；不匹配 → 删除文件、报"下载不完整或已被篡改"、允许换通道重试。
4. 只执行经 `download_app_update` 返回并校验过的绝对路径，前端不能指定任意路径安装。

## 后端设计（`src-tauri/src/commands/update.rs`，按 检查/下载/安装 分节）

**检查**：`AppUpdateInfo { version, url, notes, file, size, sha256, gitee_url, github_url }`；`url` 仍保留为 Release 页地址（兜底手动下载入口）。原 `pick_latest_app_version` / `is_app_version` 及其单测保留（dist tag 过滤仍要用）。

**下载**：`download_app_update(app) -> Result<DownloadedInfo, String>`，其中
`DownloadedInfo { path: String, version: String, size: u64, sha256: String, channel: String }`
（`path` 为落地绝对路径，只有这个结构返回过的路径才允许被安装）。

- 依次尝试 `gitee_url → github_url`；流式读 chunk 写 `%TEMP%\<file>`，边写边算 SHA-256。
- 进度经事件 `app-update-progress`：`{ phase: "downloading"|"verifying"|"done"|"failed", percent, channel, message }`，命名与载荷风格对齐既有 `plugin-setup-progress`。
- 通道判失败：90 秒内无新字节到达即视为该通道不可用，切下一条（下载成功不超时设总上限，避免慢速网络被掐断）。
- 取消：`cancel_app_update()` 置 `AtomicBool`，下载循环检查并清理半成品。
- 并发：`AtomicBool` 忙碌位防重复下载，占用中直接返回"已有下载在进行"。
-  managed 状态 `AppUpdaterState { busy, cancel }`，与字幕 `SubtitleState` 同样的注册方式。

**安装**：`install_app_update(path) -> Result<(), String>`

- 校验 path 位于本模块的下载目录（`%TEMP%`）且文件名通过上面的白名单；
- 经 `cmd /C "timeout /t 2 /nobreak >nul & start \"\" \"<exe>\" /S"` 拉起：延迟固定 2 秒，
  给本进程 `app.exit(0)` 留出释放 `voiceassist.exe` 文件占用的时间（不依赖 NSIS 自行等待）；
  `spawn` 后 drop 子进程句柄，随即 `app.exit(0)`；`cmd` 窗口用
  `CREATE_NO_WINDOW` (0x08000000) 隐藏；

风险与回退（必须真机验证）：① NSIS 是 currentUser 模式，防火墙钩子用 `ExecShellWait runas`，`/S` 静默下**仍会弹一次 UAC**（与常规安装一致，UI 需提前告知）；② 延迟启动是否足以释放文件占用。若实测不稳，去掉 `/S` 改为非静默拉起安装器（NSIS 自带占用处理），命令面与其他设计不变。

## 前端设计

- `src/components/Common/MarkdownLite.tsx`：零依赖，支持 `##`/`###`、`-` 无序列表、`**粗体**`、`` `行内代码` ``、`>` 引用、`[文本](链接)`；链接一律走 `openUrl`；样式只用安墨 token。
- `src/components/Common/CopyableGroupId.tsx`：抽出关于页现有"点击复制 + 已复制气泡"，导出 `QQ_GROUP` 常量（当前值 `690907648`），关于页与升级失败态共用。
- `src/components/Common/AppUpdater.tsx`：单一升级控件，四态 — 说明+「立即升级」／下载进度（通道+百分比）+「取消」／完成+「立即安装并重启」／失败（复制群号 + 两条直链 + 换通道重试）。点安装时若字幕监听在跑或有合成/安装任务，先二次确认。
- `stores/updateStore.ts` 增加 `downloadState / percent / channel / localPath / error` 及 `download() / cancel() / install()`。
- 两处入口收敛：启动弹窗 `Settings/UpdateDialog.tsx` 与设置「关于」页原先各有一套"前往下载"，统一改用 `<AppUpdater/>`。

## 发布侧改动

- `gitee-release.ps1`（仓库根）：导出 `Invoke-GiteeJson` / `Send-GiteeAttachment`；`publish_remote_app.ps1` 改为 dot-source 它并删除本地副本，避免 helper 双份漂移。
- 本体发布新增必做步骤：在 Gitee 建本体 Release（tag `vX.Y.Z`）并上传安装包附件，否则 Gitee 通道无数据源。
- `make-app-version.ps1`：读安装包算 sha256/size → 写 `app-version.json` → 提交进 Gitee `dist` 分支 → 作为资产附到 GitHub 本体 Release → `dist` 分支同步推 origin。
- `doc/发布流程.md` 更新：§四 资产清单加 `app-version.json`；新增"本体 Gitee Release + 附件"步骤；latest 铁律补"`app-version.json` 与 `plugins-index.json` 每次本体发布必须同时随 Release 发"。

## 测试方案

Rust 单测：清单解析（缺字段/多余字段容错）、版本比较、URL 白名单接受与拒绝、`file` 与 `version` 一致性、sha256 校验通过与篡改失败、tag 过滤既有测试保持绿。
前端：`tsc --noEmit` + `vite build`；MarkdownLite 手工对照本次 release-notes 实际文本。
真机四场景：断网 / 只通 Gitee / 只通 GitHub / 两通道全断（走群号兜底）。
端到端：先发布 v1.8.5，再本地构建一个**版本号仍写 1.8.4** 的带新逻辑包，跑完整"发现 1.8.5 → Gitee 下载 → 校验 → 安装 → 重启为 1.8.5"（因为正在运行的 1.8.4 官方包不含新逻辑，不这样做无法真机验证）。
