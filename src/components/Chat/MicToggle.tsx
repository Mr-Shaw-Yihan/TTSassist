// 工具栏「发送到麦克风」全局开关按钮。
// 开启后：每次 TTS 生成的语音除了扬声器播放，还会发到虚拟麦克风设备（队友听）。
// 开启前会检测 VB-Cable 驱动，未安装则弹出安装向导。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { checkVbCable } from "../../services/invoke";
import { VbCableInstallDialog } from "../Settings/VbCableInstallDialog";

interface Props {
  /** 点击"未配置"状态时跳转到设置页 */
  onOpenSettings: () => void;
}

export function MicToggle({ onOpenSettings }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [showInstallDialog, setShowInstallDialog] = useState(false);

  const enabled = settings?.mic_send_enabled ?? false;
  const hasDevice = !!(settings?.mic_output_device && settings.mic_output_device.trim());

  // 没配置设备时，点击改为引导去设置；开启前先检测 VB-Cable 驱动
  async function onClick() {
    if (!hasDevice) {
      onOpenSettings();
      return;
    }
    if (!enabled) {
      // 开启前检测：驱动缺失时弹出安装向导
      let installed = false;
      try {
        installed = await checkVbCable();
      } catch {
        installed = false;
      }
      if (!installed) {
        setShowInstallDialog(true);
        return;
      }
    }
    patch("mic_send_enabled", !enabled);
  }

  const title = !hasDevice
    ? "尚未配置虚拟麦克风设备，点击去设置"
    : enabled
      ? "已开启：语音会发到虚拟麦克风（点击关闭）"
      : "发送到麦克风（点击开启）";

  return (
    <>
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
      {showInstallDialog && (
        <VbCableInstallDialog
          onClose={() => setShowInstallDialog(false)}
          onInstalled={() => setShowInstallDialog(false)}
        />
      )}
    </>
  );
}
