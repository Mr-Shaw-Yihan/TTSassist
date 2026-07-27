// 工具栏「发送到麦克风」全局开关按钮。
// 开启后：每次 TTS 生成的语音除了扬声器播放，还会发到虚拟麦克风设备（队友听）。

import { useSettingsStore } from "../../stores/settingsStore";

interface Props {
  /** 点击"未配置"状态时打开设置抽屉 */
  onOpenSettings: () => void;
}

export function MicToggle({ onOpenSettings }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  const enabled = settings?.mic_send_enabled ?? false;
  const hasDevice = !!(settings?.mic_output_device && settings.mic_output_device.trim());

  // 没配置设备时，点击改为引导去设置
  function onClick() {
    if (!hasDevice) {
      onOpenSettings();
      return;
    }
    patch("mic_send_enabled", !enabled);
  }

  const title = !hasDevice
    ? "尚未配置虚拟麦克风设备，点击去设置"
    : enabled
      ? "已开启：语音会发到虚拟麦克风（点击关闭）"
      : "发送到麦克风（点击开启）";

  return (
    <button
      onClick={onClick}
      title={title}
      className={[
        "flex items-center gap-1 rounded-lg px-2 py-1.5 text-base transition-all",
        enabled && hasDevice
          ? "bg-[var(--amber-500)] text-[var(--paper)] shadow-sm"
          : hasDevice
            ? "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
            : "text-[var(--ink-200)] hover:bg-[var(--ink-100)]",
      ].join(" ")}
    >
      <span>{enabled && hasDevice ? "🎙️" : "🎤"}</span>
      {enabled && hasDevice && (
        <span className="text-[10px] font-medium tracking-wide">麦</span>
      )}
    </button>
  );
}