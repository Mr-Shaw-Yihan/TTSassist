// 收藏夹视图：列表展示 + 播放 + 删除 + 导入音频 + 自定义快捷键
// 大纲 8.1-8.4 + 阶段 15（收藏快捷键 + 冲突检测）。

import { useState } from "react";
import type { Favorite } from "../../types";
import {
  deleteFavorite,
  importFavorite,
  pickAudioFile,
  revealAudio,
  setFavoriteHotkey,
  removeFavoriteHotkey,
} from "../../services/invoke";
import { HotkeyCapture } from "./HotkeyCapture";

interface Props {
  favorites: Favorite[];
  /** 当前正在播放的 audio_path（用于高亮） */
  playingPath: string | null;
  onPlay: (audioPath: string) => void;
  onChanged: () => void;
}

export function FavoriteList({ favorites, playingPath, onPlay, onChanged }: Props) {
  // 正在录入快捷键的收藏 id
  const [capturingId, setCapturingId] = useState<string | null>(null);

  async function handleImport() {
    const filePath = await pickAudioFile();
    if (!filePath) return;
    const note = window.prompt("请输入备注（必填）：");
    if (!note || !note.trim()) return;
    try {
      await importFavorite(filePath, note.trim());
      onChanged();
    } catch (e) {
      window.alert(`导入失败：${e}`);
    }
  }

  async function handleDelete(id: string) {
    const ok = await deleteFavorite(id);
    if (ok) onChanged();
  }

  async function handleReveal(audioPath: string) {
    try {
      await revealAudio(audioPath);
    } catch (e) {
      window.alert(`无法打开文件位置：${e}`);
    }
  }

  // 设置快捷键（后端检测冲突，冲突时抛错 → 弹提示）
  async function handleSetHotkey(id: string, hotkey: string) {
    try {
      await setFavoriteHotkey(id, hotkey);
      setCapturingId(null);
      onChanged();
    } catch (e) {
      window.alert(String(e));
    }
  }

  async function handleRemoveHotkey(id: string) {
    try {
      await removeFavoriteHotkey(id);
      onChanged();
    } catch (e) {
      window.alert(String(e));
    }
  }

  if (favorites.length === 0) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-[var(--ink-300)] animate-fade">
          <span className="font-display text-3xl text-[var(--ink-200)]">·</span>
          <p className="text-sm">尚未收藏</p>
          <p className="text-xs text-[var(--ink-300)]">右键消息可收藏，或点下方导入音频</p>
        </div>
        <button
          onClick={handleImport}
          className="mb-3 self-center rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-2 text-sm text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] transition-colors"
        >
          + 导入音频
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-2 px-3 py-4">
        {favorites.map((f) => {
          const playing = playingPath === f.audio_path;
          const capturing = capturingId === f.id;
          return (
            <div key={f.id} className="animate-rise">
              <div className="group flex items-center gap-2 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5 text-sm shadow-[0_1px_2px_rgba(26,24,22,0.03)]">
                <span className="text-[var(--seal)] text-base">⌑</span>
                <span className="flex-1 break-words text-[var(--ink-900)] leading-snug">{f.note}</span>
                {/* 快捷键徽章（已设置时显示） */}
                {f.hotkey && (
                  <span
                    className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]"
                    title="快捷播放快捷键"
                  >
                    {f.hotkey}
                  </span>
                )}
                <button
                  onClick={() => onPlay(f.audio_path)}
                  className={[
                    "rounded-lg px-2 py-1 text-xs transition-colors",
                    playing
                      ? "text-[var(--amber-600)] is-playing font-medium"
                      : "text-[var(--ink-500)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-900)]",
                  ].join(" ")}
                >
                  {playing ? "♪ 播放中" : "▶ 播放"}
                </button>
                {/* 设置/更换快捷键 */}
                <button
                  onClick={() => setCapturingId(capturing ? null : f.id)}
                  className={[
                    "rounded-lg px-1.5 py-1 text-xs transition-colors",
                    capturing
                      ? "bg-[var(--ink-100)] text-[var(--ink-900)]"
                      : "text-[var(--ink-300)] opacity-0 hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)] group-hover:opacity-100",
                  ].join(" ")}
                  title="设置快捷播放快捷键"
                >
                  ⌨
                </button>
                {/* 移除快捷键（已设置时显示） */}
                {f.hotkey && (
                  <button
                    onClick={() => handleRemoveHotkey(f.id)}
                    className="rounded-lg px-1.5 py-1 text-xs text-[var(--ink-300)] opacity-0 transition-opacity hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] group-hover:opacity-100"
                    title="移除快捷键"
                  >
                    ⌫
                  </button>
                )}
                <button
                  onClick={() => handleReveal(f.audio_path)}
                  className="rounded-lg px-1.5 py-1 text-xs text-[var(--ink-300)] opacity-0 transition-opacity hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)] group-hover:opacity-100"
                  title="在文件夹中显示"
                >
                  ⧉
                </button>
                <button
                  onClick={() => handleDelete(f.id)}
                  className="rounded-lg px-1.5 py-1 text-xs text-[var(--ink-300)] opacity-0 transition-opacity hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] group-hover:opacity-100"
                  title="删除"
                >
                  ×
                </button>
              </div>
              {/* 快捷键录入（展开在该行下方） */}
              {capturing && (
                <div className="mt-1.5">
                  <HotkeyCapture
                    onCapture={(h) => handleSetHotkey(f.id, h)}
                    onCancel={() => setCapturingId(null)}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
      <button
        onClick={handleImport}
        className="mb-3 self-center rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-2 text-sm text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] transition-colors"
      >
        + 导入音频
      </button>
    </div>
  );
}