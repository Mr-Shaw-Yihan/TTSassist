// 消息气泡组件：显示内容 + 播放按钮 + 右键菜单（删除/收藏）
// 大纲 4.3-4.4
// 互斥播放（4.6.4）：组件自身不持有 audio 元素，点播放调父级 onPlay。

import { useState } from "react";
import type { Message } from "../../types";
import { deleteMessage, addFavorite, revealAudio } from "../../services/invoke";

interface Props {
  message: Message;
  /** 当前正在播放的 audio_path（跨视图统一高亮） */
  playingPath: string | null;
  onDeleted: (id: string) => void;
  onFavorited: () => void;
  onPlay: (audioPath: string) => void;
}

export function MessageBubble({ message, playingPath, onDeleted, onFavorited, onPlay }: Props) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const isPlaying = playingPath === message.audio_path;

  async function handleDelete() {
    setMenu(null);
    const ok = await deleteMessage(message.id);
    if (ok) onDeleted(message.id);
  }

  async function handleReveal() {
    setMenu(null);
    try {
      await revealAudio(message.audio_path);
    } catch (e) {
      window.alert(`无法打开文件位置：${e}`);
    }
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
    <div className="flex justify-end animate-rise">
      <div
        className="group relative max-w-[78%] border border-[var(--ink-200)] bg-[var(--paper-card)] rounded-t-2xl rounded-bl-2xl rounded-br-md px-3.5 py-2.5 text-sm text-[var(--ink-900)] shadow-[0_1px_2px_rgba(26,24,22,0.04)] transition-colors"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <div className="whitespace-pre-wrap break-words leading-[1.65]">{message.content}</div>
        <div className="mt-1.5 flex items-center gap-2 text-[11px] text-[var(--ink-300)]">
          <button
            onClick={() => onPlay(message.audio_path)}
            className={[
              "rounded-md px-1.5 py-0.5 transition-colors",
              isPlaying
                ? "text-[var(--amber-600)] is-playing font-medium"
                : "text-[var(--ink-500)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
            ].join(" ")}
          >
            {isPlaying ? "♪ 播放中" : "▶ 播放"}
          </button>
          <span className="tabular-nums">{formatTime(message.created_at)}</span>
        </div>

        {/* 右键菜单 ── 纸笺卡片 */}
        {menu && (
          <>
            <div
              className="fixed inset-0 z-40 animate-fade"
              onClick={() => setMenu(null)}
            />
            <div
              className="fixed z-50 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1 text-sm text-[var(--ink-700)] shadow-[0_8px_24px_rgba(26,24,22,0.10)] animate-fade overflow-hidden"
              style={{ left: menu.x, top: menu.y }}
            >
              <button
                onClick={handleFavorite}
                className="block w-full px-4 py-2 text-left hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)] transition-colors"
              >
                收藏到签笺
              </button>
              <button
                onClick={handleReveal}
                className="block w-full px-4 py-2 text-left hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)] transition-colors"
              >
                在文件夹中显示
              </button>
              <button
                onClick={handleDelete}
                className="block w-full px-4 py-2 text-left text-[var(--seal)] hover:bg-[var(--seal)]/10 transition-colors border-t border-[var(--ink-200)]"
              >
                删除
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