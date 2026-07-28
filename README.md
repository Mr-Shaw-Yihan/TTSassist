# TTSassist · 语笺

为语言障碍者打造的文本转语音沟通助手，特别针对游戏等全屏场景做了优化。按 `Alt+V` 在任何界面呼出浮窗，输入文字即生成语音播放。

## 特性

- **全屏浮窗**：按 `Alt+V` 呼出无边框置顶浮窗，输入文字 → 自动生成语音播放，用完即走
- **主界面聊天**：类微信聊天界面，历史消息持久化、可重复播放
- **收藏夹**：右键消息或导入外部音频，备注收藏
- **克隆音色**：导入一段本地说话音频（mp3/wav，≤10MB），MiMo 复刻相似音色
- **皮肤切换**：安墨（浅色）/ 夜窗（深色）两套
- **系统托盘**：关闭主窗最小化到托盘不退出

## 下载安装

前往 [Releases](../../releases) 下载最新版本：

- Windows 10/11：`TTSassist_1.0.0_x64-setup.exe`（NSIS 安装包，推荐）
- 免安装版：`TTSassist.exe`（双击即用）

## 首次使用

1. 启动后右上角 ⋯ → 设置
2. 选择 TTS引擎（Mimo，响应快； Moss，音色丰富）
3. 填入 **API Key**（前往 [platform.xiaomimimo.com](https://platform.xiaomimimo.com?ref=U277DH) 注册领取；可填写邀请码 `U277DH` 获得 10R 额度）
4. 输入文字、回车发送即可合成播放
5. 快捷键 alt + v 打开/关闭 浮窗，快捷输入转语音

## 技术栈

Tauri 2 + React 19 + TypeScript + Tailwind CSS v4 + Rust
TTS 采用 [小米 MiMo]、[Mossland]

## 开发

```bash
npm install
npm run tauri dev      # 开发
npm run tauri build   # 打包
```

需要 Rust 工具链（rustup）和 Windows 的 MSVC + Windows SDK。

## License

[MIT](./LICENSE) © Mr-Shaw-Yihan

## 备忘录
1. 托盘双击打开主界面、优化便捷浮窗
2. 支持本地tts引擎接入，提供更快的响应速度以及音色自定义能力