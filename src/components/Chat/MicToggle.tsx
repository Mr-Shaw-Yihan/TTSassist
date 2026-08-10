// 工具栏「发送到麦克风」全局开关按钮。
// 开启后：每次 TTS 生成的语音除了扬声器播放，还会发到虚拟麦克风设备（队友听）。
// 开启前会检测 VB-Cable 驱动，未安装则弹出安装向导。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { checkVbCable } from "../../services/invoke";
import { VbCableInstallDialog } from "../Settings/VbCableInstallDialog";
import { MicIcon } from "../icons/MicIcon";

interface Props {
  /** 点击"未配置"状态时跳转到设置页 */
  onOpenSettings: () => void;
  /** 展示形态：icon=图标按钮（默认，浮窗用）；row=图标+文字+滑块开关（「其他」面板用） */
  variant?: "icon" | "row";
}

export function MicToggle({ onOpenSettings, variant = "icon" }: Props) {
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

  const on = enabled && hasDevice;

  // 行形态：图标 + 文字居左，滑块开关居右（「其他」面板内）
  if (variant === "row") {
    return (
      <>
        <button
          onClick={onClick}
          title={title}
          className={[
            "flex w-full items-center gap-2 rounded-lg px-1.5 py-1 transition-colors hover:bg-[var(--ink-100)]",
            !hasDevice && "opacity-60",
          ].join(" ")}
        >
          <MicIcon size={14} className={on ? "text-[var(--amber-600)]" : "text-[var(--ink-300)]"} />
          <span className="flex-1 text-left text-xs text-[var(--ink-700)]">发到麦克风</span>
          {/* 滑块开关 */}
          <span
            aria-hidden
            className={[
              "relative h-4 w-7 shrink-0 rounded-full transition-colors",
              on ? "bg-[var(--amber-500)]" : "bg-[var(--ink-200)]",
            ].join(" ")}
          >
            <span
              className={[
                "absolute top-0.5 h-3 w-3 rounded-full bg-white shadow-sm transition-all",
                on ? "left-3.5" : "left-0.5",
              ].join(" ")}
            />
          </span>
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
        <MicIcon size={16} />
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
