# MiMo ASR 插件使用说明

> **MiMo ASR（小米·云端）** —— 基于小米 MiMo-V2.5-ASR 模型的语音识别插件，把你说的话变成文字。

## 这个插件是干什么的？

VoiceAssist 的核心是"打字 → 播报"。装上这个插件后，未来可以通过**语音输入**代替打字：

**对着麦克风说话 → 插件识别成文字 → 自动填入输入框**

识别由小米 MiMo-V2.5-ASR 云端模型完成，具备以下能力：

- **中英双语**：支持中文、英文识别，也可自动检测语种
- **方言支持**：原生支持粤语、吴语、闽南语、四川话等中国方言
- **复杂场景**：噪声环境、多人对话、带背景音乐的歌词也能识别
- **自动标点**：识别结果自带标点，无需后处理

## 使用前提

| 项目 | 要求 |
|------|------|
| API Key | 需要小米 MiMo 平台的 API Key（与 MiMo TTS 共用同一个 Key） |
| 网络 | 云端识别，使用时需联网 |
| 系统 | Windows |
| VoiceAssist 版本 | ≥ 1.5.0 |

### 获取 API Key

1. 访问小米 MiMo 开放平台（mimo.mi.com）注册并登录
2. 在控制台创建 API Key
3. 打开 VoiceAssist **设置页**，把 Key 填入 **MiMo API Key** 输入框并保存

> 如果你已经在使用 MiMo TTS（文字转语音），则无需重复配置，两个功能共用同一个 Key。

## 安装方法

### 方式一：一键安装（推荐，本机开发调试用）

先关闭正在运行的 VoiceAssist，然后在 `plugins/mimo-asr` 目录下执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install
```

脚本会自动：构建插件 → 生成清单（含 SHA-256 校验）→ 安装到 `<exe同级>/plugins/mimo-asr/`（阶段 22 起脱离 AppData）

### 方式二：打包后手动安装

```powershell
powershell -ExecutionPolicy Bypass -File .\package.ps1
```

会在 `dist/` 目录生成 `mimo-asr-1.0.0.zip`，可通过应用的插件管理界面安装。

安装完成后启动 VoiceAssist，插件会自动加载。

## 使用说明

### 支持的音频格式

| 格式 | 说明 |
|------|------|
| WAV | 自动识别（RIFF 头） |
| MP3 | 自动识别（ID3 / 帧头） |

### 支持的识别语言

| 代码 | 语言 |
|------|------|
| `auto` | 自动检测（默认） |
| `zh` | 中文 |
| `en` | English |

> 明确知道说什么语言时，手动指定语言可提升识别准确率。

### 音频限制

- **大小**：单次识别音频不超过 **7 MB**（约对应 40+ 分钟低码率 MP3，日常录音完全够用）
- 超过上限时插件会直接提示，不会白白消耗 API 额度

### 计费

识别调用按小米 MiMo 平台的按量计费规则收费，账单可在 MiMo 控制台查看。

## 常见问题

**Q：提示"未配置 MIMO_API_KEY"或"鉴权失败"？**
检查 VoiceAssist 设置页是否已填入有效的 MiMo API Key，Key 是否过期。修改 Key 后需要**重启应用**才会生效（Key 在启动时注入插件）。

**Q：提示"请求过于频繁（429）"？**
触发了云端限流，稍等几秒重试即可。

**Q：识别结果为空？**
可能是录音里没有人声（静音/纯噪声），或音频格式不受支持。

**Q：识别速度慢？**
云端识别耗时与音频长度正相关，几十秒的音频通常 1~3 秒返回。网络状况差时会变慢（最长等待 120 秒）。

**Q：修改设置后插件没生效？**
插件在应用启动时加载，修改 ASR 相关设置后请重启 VoiceAssist。

## 技术说明（开发者向）

- **接口**：`POST https://api.xiaomimimo.com/v1/chat/completions`（OpenAI 兼容格式）
- **模型**：`mimo-v2.5-asr`
- **鉴权**：请求头 `api-key`，插件从环境变量 `MIMO_API_KEY` 读取（宿主启动时从设置注入）
- **插件 ABI**：遵循 VoiceAssist 插件规范（`va_asr_plugin!` 宏生成导出函数），详见 `doc/ASR语音输入插件开发指南.md`

## 卸载

1. 关闭 VoiceAssist
2. 删除目录 `<exe同级>/plugins/mimo-asr/`（阶段 22 起插件在 exe 同级，不在 AppData）
3. 编辑 `<exe同级>/plugins/registry.json`，移除 `id` 为 `mimo-asr` 的条目
