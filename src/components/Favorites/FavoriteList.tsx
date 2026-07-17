// 收藏夹视图：列表展示 + 播放 + 删除 + 导入音频
// 大纲 8.1-8.4。播放与消息列表共享 App 全局 Audio（互斥播放跨视图）。

import type { Favorite } from "../../types";
import { deleteFavorite, importFavorite, pickAudioFile, revealAudio } from "../../services/invoke";

interface Props {
  favorites: Favorite[];
  /** 当前正在播放的 audio_path（用于高亮） */
  playingPath: string | null;
  onPlay: (audioPath: string) => void;
  onChanged: () => void;
}

export function FavoriteList({ favorites, playingPath, onPlay, onChanged }: Props) {
  async function handleImport() {
    const filePath = await pickAudioFile();
    if (!filePath) return; // 用户取消
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
          return (
            <div
              key={f.id}
              className="group flex items-center gap-2 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5 text-sm shadow-[0_1px_2px_rgba(26,24,22,0.03)] animate-rise"
            >
              <span className="text-[var(--seal)] text-base">⌑</span>
              <span className="flex-1 break-words text-[var(--ink-900)] leading-snug">{f.note}</span>
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