// 工具栏喇叭按钮：点击弹出音量 + 语速双滑条（复用 VolumeControl 的设置写入逻辑）。
// 点弹层外收起；主窗与浮窗工具栏共用。

import { useEffect, useRef, useState } from "react";
import { TexIcon } from "../icons/TexIcon";
import { useSettingsStore } from "../../stores/settingsStore";

export function VolumePopover() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  // 点组件外收起
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const volume = Math.round((settings?.playback_volume ?? 0.8) * 100);
  const rate = (settings?.playback_rate ?? 1.0).toFixed(1);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        title="音量与语速"
        onClick={() => setOpen((v) => !v)}
        className={[
          "flex h-8 w-8 items-center justify-center rounded-[9px] transition-colors",
          open
            ? "bg-[var(--amber-200)]/50 text-[var(--amber-600)]"
            : "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
        ].join(" ")}
      >
        <TexIcon name="speaker" size={17} />
      </button>

      {open && (
        <div className="animate-rise absolute bottom-full left-0 z-40 mb-2 w-[230px] rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-3 shadow-[0_8px_28px_rgba(26,24,22,0.13)]">
          {/* 音量 */}
          <div className="mb-2.5 flex items-center gap-2.5">
            <span className="grid w-5 shrink-0 place-items-center text-[var(--ink-300)]">
              <TexIcon name="speaker" size={14} />
            </span>
            <input
              type="range"
              min={0}
              max={100}
              value={volume}
              onChange={(e) => void patch("playback_volume", Number(e.target.value) / 100)}
              className="min-w-0 flex-1 accent-[var(--amber-500)]"
            />
            <span className="w-10 shrink-0 text-right font-mono text-[10.5px] text-[var(--ink-500)]">{volume}%</span>
          </div>
          {/* 语速 */}
          <div className="flex items-center gap-2.5">
            <span className="grid w-5 shrink-0 place-items-center text-[var(--ink-300)]">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden>
                <path d="M5 12h6M15 12h4M11 12a4 4 0 0 1 4-4M11 12a4 4 0 0 0 4 4" />
              </svg>
            </span>
            <input
              type="range"
              min={50}
              max={200}
              value={Math.round((settings?.playback_rate ?? 1.0) * 100)}
              onChange={(e) => void patch("playback_rate", Number(e.target.value) / 100)}
              className="min-w-0 flex-1 accent-[var(--amber-500)]"
            />
            <span className="w-10 shrink-0 text-right font-mono text-[10.5px] text-[var(--ink-500)]">{rate}x</span>
          </div>
        </div>
      )}
    </div>
  );
}
