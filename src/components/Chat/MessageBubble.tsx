// 消息气泡组件：显示内容 + 播放按钮 + 右键菜单（删除/收藏）
// 大纲 4.3-4.4
// 互斥播放（4.6.4）：组件自身不持有 audio 元素，点播放调父级 onPlay。

import { useState } from "react";
import type { Message } from "../../types";
import { deleteMessage, addFavorite } from "../../services/invoke";

interface Props {
  message: Message;
  /** 当前正在播放的消息 id（用于高亮"正在播放"） */
  playingId: string | null;
  onDeleted: (id: string) => void;
  onFavorited: () => void;
  onPlay: (audioPath: string) => void;
}

export function MessageBubble({ message, playingId, onDeleted, onFavorited, onPlay }: Props) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const isPlaying = playingId === message.id;

  async function handleDelete() {
    setMenu(null);
    const ok = await deleteMessage(message.id);
    if (ok) onDeleted(message.id);
  }

  async function handleFavorite() {
    setMenu(null);
    const note = window.prompt("请输入备注（必填）：");
    if (!note || !note.trim()) return; // 用户取消或留空
    try {
      await addFavorite(message.id, note.trim());
      onFavorited();
    } catch (e) {
      window.alert(`收藏失败：${e}`);
    }
  }

  return (
    <div className="flex justify-end">
      <div
        className="group relative max-w-[80%] rounded-2xl bg-blue-500 px-3 py-2 text-sm text-white shadow-sm"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <div className="whitespace-pre-wrap break-words">{message.content}</div>
        <div className="mt-1 flex items-center gap-2 text-xs text-blue-100">
          <button
            onClick={() => onPlay(message.audio_path)}
            className={[
              "rounded px-1.5 py-0.5 hover:bg-blue-600",
              isPlaying ? "bg-blue-700/60 font-semibold" : "",
            ].join(" ")}
          >
            {isPlaying ? "🔊 播放中" : "▶ 播放"}
          </button>
          <span className="opacity-70">{formatTime(message.created_at)}</span>
        </div>

        {/* 右键菜单（简单实现，点击外部关闭） */}
        {menu && (
          <>
            <div
              className="fixed inset-0 z-40"
              onClick={() => setMenu(null)}
            />
            <div
              className="fixed z-50 rounded-lg border border-gray-200 bg-white py-1 text-sm text-gray-800 shadow-lg"
              style={{ left: menu.x, top: menu.y }}
            >
              <button
                onClick={handleFavorite}
                className="block w-full px-4 py-1.5 text-left hover:bg-blue-50"
              >
                ⭐ 收藏
              </button>
              <button
                onClick={handleDelete}
                className="block w-full px-4 py-1.5 text-left text-red-600 hover:bg-red-50"
              >
                🗑 删除
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}