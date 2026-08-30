// 收藏夹视图：列表展示 + 播放 + 右键菜单（快捷键管理/定位文件/删除）+ 导入音频
// 大纲 8.1-8.4 + 阶段 15（收藏快捷键）+ 右键菜单收纳低频操作（节省行内空间）。

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
import { TexIcon } from "../icons/TexIcon";

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
  // 右键菜单（含触发位置与目标收藏 id）
  const [menu, setMenu] = useState<{ x: number; y: number; id: string } | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const menuFavorite = menu ? favorites.find((f) => f.id === menu.id) ?? null : null;

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

  // 点别处 / 右键别处 → 关闭菜单（同消息气泡菜单的做法）
  useEffect(() => {
    if (!menu) return;
    function onDown(e: MouseEvent) {
      if (menuRef.current && menuRef.current.contains(e.target as Node)) return;
      setMenu(null);
    }
    function onCtx(e: MouseEvent) {
      if (menuRef.current && menuRef.current.contains(e.target as Node)) return;
      setMenu(null);
    }
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("contextmenu", onCtx, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("contextmenu", onCtx, true);
    };
  }, [menu]);

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

  // 设置快捷键：与其它收藏冲突时先弹确认（是否解绑原收藏），后端仍做安全兜底检测
  async function handleSetHotkey(id: string, hotkey: string) {
    const conflict = favorites.find((f) => f.id !== id && f.hotkey === hotkey);
    let takeover = false;
    if (conflict) {
      takeover = window.confirm(
        `快捷键 ${hotkey} 已绑定到收藏「${conflict.note}」。\n是否解绑该收藏，改为绑定当前收藏？`,
      );
      if (!takeover) return;
    }
    try {
      await setFavoriteHotkey(id, hotkey, takeover);
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
      <div className="scrollbar-thin min-h-0 flex-1 space-y-2 overflow-y-auto px-3 py-4">
        {favorites.map((f) => {
          const playing = playingPath === f.audio_path;
          const capturing = capturingId === f.id;
          return (
            <div key={f.id} className="animate-rise">
              <div
                className="group flex items-center gap-2 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5 text-sm shadow-[0_1px_2px_rgba(26,24,22,0.03)]"
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, id: f.id });
                }}
                title="右键查看更多操作"
              >
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
                    "flex items-center gap-1 rounded-lg px-2 py-1 text-xs transition-colors",
                    playing
                      ? "text-[var(--amber-600)] is-playing font-medium"
                      : "text-[var(--ink-500)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-900)]",
                  ].join(" ")}
                >
                  <TexIcon name="play" size={12} />
                  {playing ? "播放中" : "播放"}
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

      {/* 右键菜单 ── portal 挂到 body，document 监听关闭 */}
      {menu &&
        menuFavorite &&
        createPortal(
          <div
            ref={menuRef}
            className="fixed z-[9999] min-w-[170px] overflow-hidden rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1 text-sm text-[var(--ink-700)] shadow-[0_8px_24px_rgba(26,24,22,0.18)] animate-fade"
            style={{ left: menu.x, top: menu.y }}
          >
            <button
              onClick={() => {
                setMenu(null);
                onPlay(menuFavorite.audio_path);
              }}
              className="flex w-full items-center gap-1.5 px-4 py-2 text-left transition-colors hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)]"
            >
              <TexIcon name="play" size={13} /> 播放
            </button>
            <button
              onClick={() => {
                setMenu(null);
                setCapturingId(menuFavorite.id);
              }}
              className="block w-full px-4 py-2 text-left transition-colors hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)]"
            >
              {menuFavorite.hotkey ? "⌨ 更换快捷键" : "⌨ 设置快捷键"}
            </button>
            {menuFavorite.hotkey && (
              <button
                onClick={() => {
                  setMenu(null);
                  handleRemoveHotkey(menuFavorite.id);
                }}
                className="block w-full px-4 py-2 text-left transition-colors hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)]"
              >
                ⌫ 移除快捷键
              </button>
            )}
            <button
              onClick={() => {
                setMenu(null);
                handleReveal(menuFavorite.audio_path);
              }}
              className="block w-full px-4 py-2 text-left transition-colors hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)]"
            >
              ⧉ 在文件夹中显示
            </button>
            <button
              onClick={() => {
                setMenu(null);
                handleDelete(menuFavorite.id);
              }}
              className="block w-full border-t border-[var(--ink-200)] px-4 py-2 text-left text-[var(--seal)] transition-colors hover:bg-[var(--seal)]/10"
            >
              × 删除
            </button>
          </div>,
          document.body,
        )}
    </div>
  );
}
