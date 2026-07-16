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
        <div className="flex flex-1 flex-col items-center justify-center text-sm text-gray-400">
          <p>暂无收藏</p>
          <p className="mt-1">右键消息可收藏，或点下方导入音频</p>
        </div>
        <button
          onClick={handleImport}
          className="mb-2 self-center rounded-lg bg-blue-500 px-4 py-2 text-sm font-medium text-white hover:bg-blue-600"
        >
          + 导入音频
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-2 px-3 py-3">
        {favorites.map((f) => {
          const playing = playingPath === f.audio_path;
          return (
            <div
              key={f.id}
              className="group flex items-center gap-2 rounded-xl border border-gray-200 bg-white px-3 py-2 text-sm shadow-sm"
            >
              <span className="text-amber-500">⭐</span>
              <span className="flex-1 break-words text-gray-800">{f.note}</span>
              <button
                onClick={() => onPlay(f.audio_path)}
                className={[
                  "rounded px-1.5 py-0.5 text-xs text-blue-600 hover:bg-blue-50",
                  playing ? "bg-blue-100 font-semibold" : "",
                ].join(" ")}
              >
                {playing ? "🔊 播放中" : "▶ 播放"}
              </button>
              <button
                onClick={() => handleReveal(f.audio_path)}
                className="rounded px-1.5 py-0.5 text-xs text-gray-500 opacity-0 hover:bg-gray-100 group-hover:opacity-100"
                title="在文件夹中打开"
              >
                📁
              </button>
              <button
                onClick={() => handleDelete(f.id)}
                className="rounded px-1.5 py-0.5 text-xs text-red-500 opacity-0 hover:bg-red-50 group-hover:opacity-100"
                title="删除"
              >
                🗑
              </button>
            </div>
          );
        })}
      </div>
      <button
        onClick={handleImport}
        className="mb-2 self-center rounded-lg bg-blue-500 px-4 py-2 text-sm font-medium text-white hover:bg-blue-600"
      >
        + 导入音频
      </button>
    </div>
  );
}