// 消息气泡组件：显示内容 + 播放按钮 + 右键菜单（删除/收藏）
// 大纲 4.3-4.4

import { useState, useRef, useEffect } from "react";
import type { Message } from "../../types";
import { getAudioUrl, deleteMessage, addFavorite } from "../../services/invoke";

interface Props {
  message: Message;
  volume: number;
  /** 播放速度（HTMLAudioElement.playbackRate） */
  playbackRate: number;
  onDeleted: (id: string) => void;
  onFavorited: () => void;
  /** 父级负责的自动播放触发器（可选） */
  autoPlaySignal?: number;
}

export function MessageBubble({ message, volume, playbackRate, onDeleted, onFavorited }: Props) {
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  // 解析音频 URL（相对路径 → asset:// 协议 URL）
  useEffect(() => {
    let cancelled = false;
    getAudioUrl(message.audio_path).then((url) => {
      if (!cancelled) setAudioUrl(url);
    });
    return () => { cancelled = true; };
  }, [message.audio_path]);

  // 应用音量与播放速度
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = volume;
      audioRef.current.playbackRate = playbackRate;
    }
  }, [volume, playbackRate, audioUrl]);

  function play() {
    audioRef.current?.play().catch(() => {/* 用户可能的拒绝权限，忽略 */});
  }

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
            onClick={play}
            className="rounded px-1.5 py-0.5 hover:bg-blue-600"
            disabled={!audioUrl}
          >
            ▶ 播放
          </button>
          <span className="opacity-70">{formatTime(message.created_at)}</span>
        </div>
        {audioUrl && (
          <audio ref={audioRef} src={audioUrl} preload="none" />
        )}

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