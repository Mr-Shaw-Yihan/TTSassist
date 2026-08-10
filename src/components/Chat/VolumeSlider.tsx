// 音量控件：默认右上角按钮形式，点击收/展含音量+播放速度两个滑块的卡片；
// inline 模式（「其他」面板内嵌）则直接铺开两个滑块，不再套二层按钮。
// 点按钮外区域收回。不再支持拖拽（悬浮球方案用户反馈拖不动，改简单）。

import { useState, useRef, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";

export function VolumeControl({ inline = false }: { inline?: boolean }) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const volume = settings?.playback_volume ?? 0.8;
  const rate = settings?.playback_rate ?? 1.0;
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  // 点外面收起（仅按钮模式用；inline 时 open 恒 false，监听不会挂上）
  useEffect(() => {
    if (!open) return;
    function onClick(e: MouseEvent) {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  // inline：直接铺滑块（外层弹层负责显隐与点外收起）
  if (inline) {
    return (
      <div className="flex flex-col gap-3 px-1 py-1">
        <Slider
          icon="音量"
          value={volume}
          min={0}
          max={1}
          step={0.05}
          format={(v) => `${Math.round(v * 100)}`}
          onChange={(v) => patch("playback_volume", v)}
        />
        <Slider
          icon="语速"
          value={rate}
          min={0.5}
          max={2}
          step={0.1}
          format={(v) => `${v.toFixed(1)}x`}
          onChange={(v) => patch("playback_rate", v)}
        />
      </div>
    );
  }

  return (
    <div ref={wrapRef} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className={[
          "rounded-lg p-1.5 text-base transition-colors",
          open
            ? "bg-[var(--amber-200)]/40 text-[var(--amber-600)]"
            : "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
        ].join(" ")}
        title="音量与播放速度"
      >
        ♪
      </button>

      {open && (
        <div className="absolute right-0 top-[calc(100%+8px)] z-50 flex flex-col gap-3 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-4 shadow-[0_8px_24px_rgba(26,24,22,0.10)] animate-fade">
          <Slider
            icon="音量"
            value={volume}
            min={0}
            max={1}
            step={0.05}
            format={(v) => `${Math.round(v * 100)}`}
            onChange={(v) => patch("playback_volume", v)}
          />
          <Slider
            icon="语速"
            value={rate}
            min={0.5}
            max={2}
            step={0.1}
            format={(v) => `${v.toFixed(1)}x`}
            onChange={(v) => patch("playback_rate", v)}
          />
        </div>
      )}
    </div>
  );
}

function Slider({
  icon,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  icon: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-[10px] text-[var(--ink-300)] tracking-wider uppercase">
        <span>{icon}</span>
        <span className="tabular-nums text-[var(--ink-500)]">{format(value)}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-32 h-[3px] cursor-pointer appearance-none rounded-full bg-[var(--ink-200)] accent-[var(--amber-600)]"
      />
    </div>
  );
}