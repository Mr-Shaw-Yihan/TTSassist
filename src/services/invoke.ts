// Tauri 命令调用封装。
// 后端每个 #[tauri::command] 对应一个函数，参数名与后端 fn 参数 snake_case 一致。

import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { Message, Favorite, Settings, MossVoice, AudioDevice, MicStatus, PluginInfo, PluginIndexEntry, BundledPluginInfo, UpdateInfo } from "../types";

// ── TTS ──────────────────────────────────────────

export async function generateTTS(text: string): Promise<Message> {
  return invoke<Message>("generate_tts", { text });
}

// ── 插件 ─────────────────────────────────────────

/** 列出已安装插件（含加载状态、失败原因、音色表） */
export async function listPlugins(): Promise<PluginInfo[]> {
  return invoke<PluginInfo[]>("list_plugins");
}

/** 卸载插件，返回提示文案（是否需重启等） */
export async function uninstallPlugin(id: string): Promise<string> {
  return invoke<string>("uninstall_plugin", { id });
}

/** 拖入安装：本地插件 zip 路径，返回提示文案 */
export async function installPluginZip(path: string): Promise<string> {
  return invoke<string>("install_plugin_zip", { path });
}

/** 拉取官方插件索引 */
export async function fetchPluginIndex(): Promise<PluginIndexEntry[]> {
  return invoke<PluginIndexEntry[]>("fetch_plugin_index");
}

/** 在线安装：下载 zip → SHA-256 校验 → 安装，返回提示文案 */
export async function downloadInstallPlugin(id: string): Promise<string> {
  return invoke<string>("download_install_plugin", { id });
}

/** 列出随安装包内置的插件（插件库） */
export async function listBundledPlugins(): Promise<BundledPluginInfo[]> {
  return invoke<BundledPluginInfo[]>("list_bundled_plugins");
}

/** 安装内置插件，返回提示文案 */
export async function installBundledPlugin(id: string): Promise<string> {
  return invoke<string>("install_bundled_plugin", { id });
}

/** 执行插件环境安装（本地引擎下载运行环境/模型）。
 *  options：JSON 字符串，如 {"voice":"mika"} 指定要确保的音色。
 *  进度经 plugin-setup-progress 事件推送；返回结果文案。 */
export async function runPluginSetup(id: string, options?: string): Promise<string> {
  return invoke<string>("run_plugin_setup", { id, options: options ?? null });
}

/** 安装指定音色（预置音色首次联网下载；环境未就绪会先补环境）。
 *  进度经 plugin-setup-progress 事件推送（kind="voice"）；返回结果文案。 */
export async function installVoice(id: string, voiceId: string): Promise<string> {
  return invoke<string>("install_voice", { id, voiceId });
}

/** 卸载指定音色（删本地音色包；服务端在跑会先释放内存）。返回结果文案。 */
export async function uninstallVoice(id: string, voiceId: string): Promise<string> {
  return invoke<string>("uninstall_voice", { id, voiceId });
}

/** 预加载已安装音色到内存（切换音色时调用，秒级；不触发下载）。
 *  有安装任务进行时后端直接跳过（仍返回成功）。 */
export async function preloadVoice(id: string, voiceId: string): Promise<string> {
  return invoke<string>("preload_voice", { id, voiceId });
}

/** 导入用户自备音色包目录（插件校验布局后复制进数据目录，保留原文件）。 */
export async function importVoicePack(id: string, srcDir: string): Promise<string> {
  return invoke<string>("import_voice_pack", { id, srcDir });
}

// ── 版本更新 ─────────────────────────────────────

/** 检查新版本；无更新或网络失败返回 null */
export async function checkAppUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_app_update");
}

// ── Messages ──────────────────────────────────────

export async function listMessages(): Promise<Message[]> {
  return invoke<Message[]>("list_messages");
}

export async function deleteMessage(id: string): Promise<boolean> {
  return invoke<boolean>("delete_message", { id });
}

// ── Favorites ────────────────────────────────────

export async function listFavorites(): Promise<Favorite[]> {
  return invoke<Favorite[]>("list_favorites");
}

export async function addFavorite(
  sourceMessageId: string,
  note: string,
): Promise<Favorite> {
  return invoke<Favorite>("add_favorite", {
    sourceMessageId,
    note,
  });
}

export async function deleteFavorite(id: string): Promise<boolean> {
  return invoke<boolean>("delete_favorite", { id });
}

/** 导入外部音频文件为收藏 */
export async function importFavorite(
  filePath: string,
  note: string,
): Promise<Favorite> {
  return invoke<Favorite>("import_favorite", { filePath, note });
}

/** 为收藏设置快捷键（后端检测冲突，冲突时抛错） */
export async function setFavoriteHotkey(
  id: string,
  hotkey: string,
): Promise<Favorite[]> {
  return invoke<Favorite[]>("set_favorite_hotkey", { id, hotkey });
}

/** 移除收藏的快捷键 */
export async function removeFavoriteHotkey(id: string): Promise<Favorite[]> {
  return invoke<Favorite[]>("remove_favorite_hotkey", { id });
}

// ── 克隆音色 ─────────────────────────────────────

export async function importCloneVoice(
  filePath: string,
  name: string,
): Promise<void> {
  return invoke<void>("import_clone_voice", { filePath, name });
}

export async function removeCloneVoice(): Promise<void> {
  return invoke<void>("remove_clone_voice");
}

// ── 文件选择 ────────────────────────────────────

import { open } from "@tauri-apps/plugin-dialog";

/** 弹文件选择器，选单个音频文件；返回绝对路径或 null（用户取消） */
export async function pickAudioFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    filters: [{ name: "音频", extensions: ["mp3", "wav", "m4a", "ogg", "flac"] }],
  });
  if (typeof selected === "string") return selected;
  return null;
}

/** 弹文件夹选择器，选自定义音色包目录；返回绝对路径或 null（用户取消） */
export async function pickVoicePackFolder(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: true,
    title: "选择音色包文件夹（内含 tts_models/ 与 prompt_wav.json）",
  });
  if (typeof selected === "string") return selected;
  return null;
}

// ── Settings ─────────────────────────────────────

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function updateSetting(
  key: string,
  value: string | number | boolean | MossVoice[] | Record<string, string>,
): Promise<Settings> {
  return invoke<Settings>("update_setting", { key, value });
}

// ── 音频播放 ─────────────────────────────────────

/**
 * 把后端返回的相对音频路径（如 "audio/m_xxx.wav"）转换成可在 <audio> 中播放的 URL。
 *
 * 调用后端的 resolve_audio_url 命令拿到绝对路径，再用 Tauri 的 convertFileSrc
 * 把绝对路径转成 asset://（Win/Mac）协议 URL，供前端 <audio> 直接播放。
 */
export async function getAudioUrl(relPath: string): Promise<string> {
  const absPath = await invoke<string>("resolve_audio_url", { relPath });
  return convertFileSrc(absPath);
}

/**
 * 拿音频的本地绝对路径（不经 convertFileSrc），供 revealItemInDir 等需要路径的场景用。
 */
export async function getAudioAbsPath(relPath: string): Promise<string> {
  return invoke<string>("resolve_audio_url", { relPath });
}

// ── 打开文件位置 ──────────────────────────────────

import { revealItemInDir } from "@tauri-apps/plugin-opener";

/** 在系统资源管理器中定位并选中该音频文件。 */
export async function revealAudio(relPath: string): Promise<void> {
  const abs = await getAudioAbsPath(relPath);
  await revealItemInDir(abs);
}

// ── 虚拟麦克风 ──────────────────────────────────

/** 枚举音频输出设备（含 VB-CABLE 标记） */
export async function listMicDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_mic_devices");
}

/** 检测是否安装 VB-CABLE */
export async function checkVbCable(): Promise<boolean> {
  return invoke<boolean>("check_vb_cable");
}

/** 手动播放音频到指定设备（传相对路径，内部解析为绝对路径） */
export async function playToMic(relPath: string, deviceName: string, volume?: number): Promise<void> {
  const abs = await getAudioAbsPath(relPath);
  return invoke<void>("play_to_mic", { audioPath: abs, deviceName, volume });
}

/** 播放测试音（440Hz）到指定设备，诊断设备路由 */
export async function testMic(deviceName: string, volume?: number): Promise<void> {
  return invoke<void>("test_mic", { deviceName, volume });
}

/** 停止虚拟麦克风播放 */
export async function stopMic(): Promise<void> {
  return invoke<void>("stop_mic");
}

/** 获取虚拟麦克风播放状态 */
export async function getMicStatus(): Promise<MicStatus> {
  return invoke<MicStatus>("get_mic_status");
}

// ── 全局快捷键 ──────────────────────────────────

/** 设置（更换）浮窗呼出快捷键，后端验证有效性并重新注册 */
export async function setHotkey(accel: string): Promise<void> {
  return invoke<void>("set_hotkey", { accel });
}

// ── VB-CABLE 驱动下载与安装 ───────────────────

/** 下载 VB-CABLE 驱动包（返回下载的 zip 路径），进度通过事件推送 */
export async function downloadVbCable(): Promise<string> {
  return invoke<string>("download_vb_cable");
}

/** 解压并启动 VB-CABLE 安装程序（需管理员权限） */
export async function installVbCable(zipPath: string): Promise<string> {
  return invoke<string>("install_vb_cable", { zipPath });
}