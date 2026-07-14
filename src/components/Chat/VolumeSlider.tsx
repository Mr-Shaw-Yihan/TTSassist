// 工具栏：播放音量 + 播放速度 + 合成语速
// 大纲 4.5：播放音量/速度由前端 HTMLAudioElement 控制；
//          合成语速塞进 MiMo user 消息（伪精确）。

import { useSettingsStore } from "../../stores/settingsStore";

/** 单个滑块（音量/速度共用） */
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
        className="w-20 accent-blue-500"
      />
      <span className="w-9 text-xs tabular-nums text-gray-500">{format(value)}</span>
    </div>
  );
}

export function Toolbar() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const volume = settings?.playback_volume ?? 0.8;
  const rate = settings?.playback_rate ?? 1.0;
  const ttsSpeed = settings?.tts_speed ?? 1.0;

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-gray-600">
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
      <Slider
        icon="🚀"
        value={ttsSpeed}
        min={0.5}
        max={2}
        step={0.1}
        format={(v) => `${v.toFixed(1)}x`}
        onChange={(v) => patch("tts_speed", v)}
      />
    </div>
  );
}