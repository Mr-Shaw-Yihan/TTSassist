// 语音输入设置：ASR 插件选择 + 识别语言 + 录音输入设备。
// 设备枚举说明：浏览器只在授予麦克风权限后才返回设备名（label），
// 未授权时显示「授权麦克风」按钮引导用户开启。

import { useState, useEffect, useCallback } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { listAsrPlugins } from "../../services/invoke";
import type { AsrPluginInfo } from "../../types";

/** enumerateDevices 拿到的输入设备（只留我们需要的字段） */
interface InputDevice {
  deviceId: string;
  label: string;
}

export function VoiceInputSettings() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  const [asrPlugins, setAsrPlugins] = useState<AsrPluginInfo[]>([]);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [hasPermission, setHasPermission] = useState(false);
  const [requesting, setRequesting] = useState(false);

  /** 枚举麦克风设备；label 非空说明已有权限 */
  const refreshDevices = useCallback(async () => {
    try {
      const all = await navigator.mediaDevices.enumerateDevices();
      const inputs = all
        .filter((d) => d.kind === "audioinput")
        .map((d) => ({ deviceId: d.deviceId, label: d.label }));
      setDevices(inputs);
      setHasPermission(inputs.some((d) => d.label));
    } catch {
      /* 枚举失败静默（极少见） */
    }
  }, []);

  useEffect(() => {
    listAsrPlugins().then(setAsrPlugins).catch(() => {});
    refreshDevices();
    // 设备热插拔时刷新列表
    navigator.mediaDevices?.addEventListener("devicechange", refreshDevices);
    return () => navigator.mediaDevices?.removeEventListener("devicechange", refreshDevices);
  }, [refreshDevices]);

  /** 授权麦克风：短暂打开一次默认设备拿权限，立即释放 */
  async function requestPermission() {
    setRequesting(true);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((t) => t.stop());
      await refreshDevices();
    } catch {
      window.alert("麦克风授权失败：请检查系统设置中是否允许本应用使用麦克风");
    } finally {
      setRequesting(false);
    }
  }

  // 当前选中插件的语言列表（解析插件上报的 languages JSON）
  const selectedPlugin = asrPlugins.find((p) => p.id === settings?.asr_plugin);
  const languages: { code: string; label: string }[] = (() => {
    try {
      return JSON.parse(selectedPlugin?.languages ?? "[]");
    } catch {
      return [];
    }
  })();

  const loadedPlugins = asrPlugins.filter((p) => p.loaded);

  return (
    <div className="space-y-3">
      {/* ASR 插件选择 */}
      <div>
        <label className="mb-1 block text-[11px] text-[var(--ink-300)]">识别插件</label>
        {loadedPlugins.length === 0 ? (
          <p className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-[11px] text-[var(--ink-500)]">
            暂无可用的语音识别插件，请先在插件页安装（如 MiMo ASR）。
          </p>
        ) : (
          <select
            value={settings?.asr_plugin ?? ""}
            onChange={(e) => patch("asr_plugin", e.target.value)}
            className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
          >
            {!selectedPlugin && <option value="">自动选择（第一个可用插件）</option>}
            {loadedPlugins.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} v{p.version}
              </option>
            ))}
          </select>
        )}
      </div>

      {/* 识别语言 */}
      {languages.length > 0 && (
        <div>
          <label className="mb-1 block text-[11px] text-[var(--ink-300)]">识别语言</label>
          <select
            value={settings?.asr_language ?? "auto"}
            onChange={(e) => patch("asr_language", e.target.value)}
            className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
          >
            {languages.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
          <p className="mt-1 text-[10px] text-[var(--ink-300)]">明确指定语言可提升识别准确率</p>
        </div>
      )}

      {/* 输入设备 */}
      <div>
        <label className="mb-1 block text-[11px] text-[var(--ink-300)]">输入设备（录音麦克风）</label>
        {hasPermission ? (
          <select
            value={settings?.voice_input_device ?? ""}
            onChange={(e) => patch("voice_input_device", e.target.value)}
            className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
          >
            <option value="">系统默认麦克风</option>
            {devices.map((d) => (
              <option key={d.deviceId} value={d.deviceId}>
                {d.label || "未命名设备"}
              </option>
            ))}
          </select>
        ) : (
          <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5 text-[11px] leading-relaxed text-[var(--ink-500)]">
            <p>查看设备列表需要先授权麦克风权限。</p>
            <button
              onClick={requestPermission}
              disabled={requesting}
              className="mt-1.5 rounded-md bg-[var(--amber-500)] px-2.5 py-1 font-medium text-[var(--paper)] transition-colors hover:bg-[var(--amber-600)] disabled:opacity-50"
            >
              {requesting ? "授权中…" : "🎙️ 授权麦克风"}
            </button>
          </div>
        )}
        <p className="mt-1 text-[10px] text-[var(--ink-300)]">
          所选设备仅用于语音输入录音；设备拔插后会自动回退到系统默认麦克风。
        </p>
      </div>
    </div>
  );
}
