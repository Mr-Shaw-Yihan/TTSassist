// 音量调节滑块（播放音量，前端控制，持久化到 settings.json）
// 大纲 4.5：音量不含 TTS 生成，仅影响前端 <audio> 播放响度。

import { useSettingsStore } from "../../stores/settingsStore";

export function VolumeSlider() {
  const settings = useSettingsStore((s) => s.settings);
  const increase_volume = useSettingsStore((s) => s.patch);
  const volume = settings?.playback_volume ?? 0.8;

  return (
    <div className="flex items-center gap-2 text-gray-500">
      <span className="text-sm">🔊</span>
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={volume}
        onChange={(e) => increase_volume("playback_volume", parseFloat(e.target.value))}
        className="w-28 accent-blue-500"
      />
      <span className="w-8 text-xs tabular-nums">{Math.round(volume * 100)}%</span>
    </div>
  );
}