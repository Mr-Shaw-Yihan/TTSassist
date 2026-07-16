// Tauri 命令调用封装。
// 后端每个 #[tauri::command] 对应一个函数，参数名与后端 fn 参数 snake_case 一致。

import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { Message, Favorite, Settings } from "../types";

// ── TTS ──────────────────────────────────────────

export async function generateTTS(text: string): Promise<Message> {
  return invoke<Message>("generate_tts", { text });
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

// ── Settings ─────────────────────────────────────

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function updateSetting(
  key: string,
  value: string | number | boolean,
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