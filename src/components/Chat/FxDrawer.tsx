// 效果器抽屉：输入行「发」左侧的一键切换入口（设计 §六 UI-2）。
//
// 交互铁律：
// - hover 展开 + click 也能展开（两条都必须有——只做 hover 键盘用户完全不可达，
//   触屏/WebView 的 hover 语义也不可靠）
// - 列表 ArrowUp/Down + Enter 可达，Esc 关闭
// - 未安装任何 audio_effect 插件时按钮整个不渲染（不是灰掉）
// - 「原声」固定首项且不可删除；当前选择有可见反馈

import { useEffect, useRef, useState } from "react";
import { listFxPresets } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import type { FxPresetInfo } from "../../types";
import { FX_PRESET_OFF, currentFxLabel, sortFxPresets } from "../../utils/fx";

interface Entry {
  key: string;
  name: string;
  description: string;
}

export function FxDrawer() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [presets, setPresets] = useState<FxPresetInfo[] | null>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const closeTimer = useRef<number | null>(null);

  useEffect(() => {
    let alive = true;
    listFxPresets()
      .then((p) => alive && setPresets(p))
      .catch(() => alive && setPresets([]));
    return () => {
      alive = false;
    };
  }, []);

  // 点外部关闭（click 展开后）
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // 有插件才渲染；没有插件时连按钮都不出现
  if (presets === null || presets.length === 0) return null;

  const order = settings?.fx_order ?? [];
  const current = settings?.fx_preset ?? "off";
  const entries: Entry[] = [
    { key: FX_PRESET_OFF, name: "原声", description: "不做效果处理" },
    ...sortFxPresets(presets, order).map((p) => ({
      key: p.key,
      name: p.name,
      description: p.description,
    })),
  ];

  const openList = () => {
    if (closeTimer.current) {
      clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
    setActive(Math.max(0, entries.findIndex((e) => e.key === current)));
    setOpen(true);
  };
  const scheduleClose = () => {
    // click 固定展开时不因鼠标离开关闭
    if (closeTimer.current) clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => setOpen(false), 250);
  };
  const choose = (key: string) => {
    void patch("fx_preset", key);
    setOpen(false);
  };
  const onListKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(entries.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(entries[active].key);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  return (
    <div
      ref={rootRef}
      className="relative"
      onMouseEnter={openList}
      onMouseLeave={scheduleClose}
    >
      <button
        type="button"
        aria-label="语音效果器"
        aria-expanded={open}
        className={[
          "flex items-center gap-1.5 rounded-xl border px-3 py-2.5 text-sm transition-colors",
          current === FX_PRESET_OFF
            ? "border-[var(--ink-200)] text-[var(--ink-500)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
            : "border-[var(--amber-600)]/40 bg-[var(--amber-600)]/8 text-[var(--amber-600)] hover:bg-[var(--amber-600)]/12",
        ].join(" ")}
        onClick={() => (open ? setOpen(false) : openList())}
        onKeyDown={(e) => {
          // 键盘通道：ArrowDown/Enter 从按钮进入列表
          if (e.key === "ArrowDown" || (e.key === "Enter" && !open)) {
            e.preventDefault();
            openList();
            window.setTimeout(() => listRef.current?.focus(), 0);
          }
        }}
      >
        {/* 声波图标 */}
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path
            d="M2 6v4M5.5 3.5v9M9 5v6M12.5 2.5v11"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
        <span className="max-w-[72px] truncate">{currentFxLabel(current, presets)}</span>
      </button>

      {open && (
        <ul
          ref={listRef}
          tabIndex={-1}
          role="listbox"
          aria-label="选择语音效果"
          onKeyDown={onListKeyDown}
          className="absolute bottom-full left-0 z-40 mb-2 w-64 max-h-72 overflow-y-auto rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-1 shadow-lg outline-none animate-rise"
        >
          {entries.map((e, i) => {
            const selected = e.key === current;
            return (
              <li key={e.key} role="option" aria-selected={selected}>
                <button
                  type="button"
                  tabIndex={-1}
                  className={[
                    "flex w-full flex-col items-start rounded-lg px-3 py-2 text-left transition-colors",
                    i === active ? "bg-[var(--ink-100)]" : "",
                    selected
                      ? "text-[var(--amber-600)]"
                      : "text-[var(--ink-700)] hover:bg-[var(--ink-100)]",
                  ].join(" ")}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => choose(e.key)}
                >
                  <span className="flex w-full items-center gap-1.5 text-sm font-medium">
                    {e.name}
                    {selected && <span className="text-xs">●</span>}
                  </span>
                  {e.description && (
                    <span className="text-[11px] text-[var(--ink-300)]">{e.description}</span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
