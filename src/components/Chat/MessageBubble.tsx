// 消息气泡组件：显示内容 + 播放按钮 + 右键菜单（删除/收藏）
// 大纲 4.3-4.4
// 互斥播放（4.6.4）：组件自身不持有 audio 元素，点播放调父级 onPlay。

import { useState, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
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
  const menuRef = useRef<HTMLDivElement | null>(null);
  const isPlaying = playingPath === message.audio_path;

  // 菜单出现时校正位置，避免溢出视窗右/下边
  useEffect(() => {
    if (!menu || !menuRef.current) return;
    const el = menuRef.current;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let x = menu.x;
    let y = menu.y;
    if (x + rect.width > vw - 8) x = Math.max(8, vw - rect.width - 8);
    if (y + rect.height > vh - 8) y = Math.max(8, vh - rect.height - 8);
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
  }, [menu]);

  // 不用全屏遮罩——改为在 document 上监听，点别处或右键别处时关闭本菜单。
  // 这样右键事件能正常落到目标气泡上，不会被遮罩拦截。
  useEffect(() => {
    if (!menu) return;
    function onDown(e: MouseEvent) {
      // 点到菜单本身不关
      if (menuRef.current && menuRef.current.contains(e.target as Node)) return;
      setMenu(null);
    }
    function onCtx(e: MouseEvent) {
      // 右键发生在菜单内不处理（让按钮可被右键）
      if (menuRef.current && menuRef.current.contains(e.target as Node)) return;
      // 右键别处时关本菜单；不 preventDefault，让右键按原本路径派发到目标气泡
      setMenu(null);
    }
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("contextmenu", onCtx, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("contextmenu", onCtx, true);
    };
  }, [menu]);

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
    <div className="flex justify-end">
      <div
        className="group relative max-w-[78%] border border-[var(--ink-200)] bg-[var(--paper-card)] rounded-t-2xl rounded-bl-2xl rounded-br-md px-3.5 py-2.5 text-sm text-[var(--ink-900)] shadow-[0_1px_2px_rgba(26,24,22,0.04)] transition-colors animate-fade"
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

        {/* 右键菜单 ── 经 portal 挂到 body，脱离气泡堆叠上下文。
            不用全屏遮罩（会拦截右键落到其它气泡上），改由 document 监听关闭。 */}
        {menu && createPortal(
          <div
            ref={menuRef}
            className="fixed z-[9999] min-w-[160px] rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1 text-sm text-[var(--ink-700)] shadow-[0_8px_24px_rgba(26,24,22,0.18)] animate-fade overflow-hidden"
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
          </div>,
          document.body,
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