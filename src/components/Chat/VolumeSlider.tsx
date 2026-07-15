// 音量控件（右上角按钮形式）：点击按钮收/展含音量+播放速度两个滑块的卡片。
// 点按钮外区域收回。不再支持拖拽（悬浮球方案用户反馈拖不动，改简单）。

import { useState, useRef, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";

export function VolumeControl() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const volume = settings?.playback_volume ?? 0.8;
  const rate = settings?.playback_rate ?? 1.0;
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  // 点外面收起
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

  return (
    <div ref={wrapRef} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className={[
          "rounded px-2 py-1 text-base",
          open ? "bg-blue-50 text-blue-600" : "text-gray-500 hover:bg-gray-100",
        ].join(" ")}
        title="音量与播放速度"
      >
        🔊
      </button>

      {open && (
        <div className="absolute right-0 top-[calc(100%+6px)] z-50 flex flex-col gap-2 rounded-xl border border-gray-200 bg-white p-3 shadow-lg">
          <Slider
            icon="🔊"
            value={volume}
            min={0}
            max={1}
            step={0.05}
            format={(v) => `${Math.round(v * 100)}%`}
            onChange={(v) => patch("playback_volume", v)}
          />
          <Slider
            icon="⚡"
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
    <div className="flex items-center gap-1.5">
      <span className="text-sm">{icon}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-24 accent-blue-500"
      />
      <span className="w-10 text-xs tabular-nums text-gray-500">{format(value)}</span>
    </div>
  );
}