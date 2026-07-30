// 虚拟麦克风设置（独立组件）：设备选择 + 音量 + 测试 + VB-CABLE 引导。
// 轮询隔离在此组件内——只有该分类展开（组件挂载）时才轮询，收起即停止，消除设置页卡顿。

import { useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { listMicDevices, checkVbCable, testMic, getMicStatus } from "../../services/invoke";
import type { AudioDevice, MicStatus } from "../../types";

export function MicSettings() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [micDevices, setMicDevices] = useState<AudioDevice[]>([]);
  const [vbInstalled, setVbInstalled] = useState(true);
  const [micStatus, setMicStatus] = useState<MicStatus | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    listMicDevices().then(setMicDevices).catch(() => {});
    checkVbCable().then(setVbInstalled).catch(() => {});
  }, []);

  // 轮询麦克风状态（仅本组件挂载时，即分类展开时）
  useEffect(() => {
    let alive = true;
    const timer = setInterval(async () => {
      if (!alive) return;
      try {
        setMicStatus(await getMicStatus());
      } catch { /* ignore */ }
    }, 600);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  async function runMicTest() {
    const device = settings?.mic_output_device ?? "";
    if (!device) return;
    setTesting(true);
    try {
      await testMic(device, settings?.mic_playback_volume ?? 1.0);
      setTimeout(() => setTesting(false), 400);
    } catch (e) {
      setMicStatus({ is_playing: false, current_device: device, volume: 1, last_error: String(e), last_source: null });
      setTesting(false);
    }
  }

  return (
    <div>
      {!vbInstalled ? (
        <div className="mb-2 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2.5 text-[11px] leading-relaxed text-[var(--amber-600)]">
          <p className="font-medium">未检测到 VB-CABLE 虚拟声卡</p>
          <p className="mt-1">要让队友听到语音，需先安装 VB-CABLE 驱动：</p>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
            <a
              href="https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 rounded-md bg-[var(--amber-500)] px-2 py-1 font-medium text-[var(--paper)] no-underline transition-colors hover:bg-[var(--amber-600)]"
            >
              ⬇ 下载驱动包
            </a>
            <a href="https://vb-audio.com/Cable/" target="_blank" rel="noreferrer"
               className="underline underline-offset-2 hover:text-[var(--seal)] transition-colors">
              或访问官网
            </a>
          </div>
          <p className="mt-1.5 text-[10px] text-[var(--amber-600)]/80">
            解压后右键以管理员身份运行 VBCABLE_Setup_x64.exe，装完重启电脑。
          </p>
          <p className="mt-1 text-[10px] text-[var(--ink-300)]">
            VB-CABLE 是捐赠软件（donationware），来源 www.vb-cable.com，欢迎向作者捐赠。
          </p>
        </div>
      ) : (
        <p className="mb-2 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-[11px] text-[var(--ink-500)]">
          ✓ 已检测到 VB-CABLE 虚拟声卡
        </p>
      )}
      <label className="mb-1 block text-[11px] text-[var(--ink-300)]">输出设备（选 CABLE Input）</label>
      <select
        value={settings?.mic_output_device ?? ""}
        onChange={(e) => patch("mic_output_device", e.target.value)}
        className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
      >
        <option value="">未配置</option>
        {[...micDevices]
          .sort((a, b) => Number(b.is_virtual_cable) - Number(a.is_virtual_cable))
          .map((d) => (
            <option key={d.name} value={d.name}>
              {d.name}{d.is_virtual_cable ? "（虚拟声卡）" : ""}{d.is_default ? "（默认）" : ""}
            </option>
          ))}
      </select>
      <div className="mt-2.5 flex items-center gap-2">
        <span className="text-[11px] text-[var(--ink-300)]">麦克风音量</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={settings?.mic_playback_volume ?? 1.0}
          onChange={(e) => patch("mic_playback_volume", parseFloat(e.target.value))}
          className="flex-1 accent-[var(--amber-600)]"
        />
        <span className="w-8 text-[11px] tabular-nums text-[var(--ink-500)]">
          {Math.round((settings?.mic_playback_volume ?? 1.0) * 100)}%
        </span>
      </div>
      <div className="mt-2.5 flex items-center gap-2">
        <button
          onClick={runMicTest}
          disabled={testing || !(settings?.mic_output_device)}
          className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-1.5 text-xs text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:cursor-not-allowed disabled:opacity-40"
        >
          {testing ? "测试中…" : "🔊 测试麦克风"}
        </button>
        <span className="text-[11px] text-[var(--ink-300)]">播放 1.2 秒测试音到所选设备</span>
      </div>
      {micStatus && (
        <div className="mt-1.5 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-[11px] leading-relaxed">
          {micStatus.last_error ? (
            <span className="text-[var(--seal)]">✗ {micStatus.last_error}</span>
          ) : micStatus.is_playing ? (
            <span className="text-[var(--amber-600)]">
              🔊 正在播放到「{micStatus.current_device}」…（{micStatus.last_source}）
            </span>
          ) : micStatus.last_source ? (
            <span className="text-[var(--ink-500)]">
              ✓ 上次已发送到「{micStatus.current_device}」（{micStatus.last_source}）
            </span>
          ) : (
            <span className="text-[var(--ink-300)]">空闲。点「测试麦克风」或发一条消息试试。</span>
          )}
          {micStatus.last_source && !micStatus.last_error && (
            <div className="mt-1 text-[var(--ink-300)]">
              若此处显示已发送但对方听不到：请确认通话软件麦克风设为「CABLE Output」。
            </div>
          )}
        </div>
      )}
      <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
        开启工具栏🎙️开关后，发送的语音会发到所选设备。请在游戏/通话软件里把麦克风设为「CABLE Output」。
      </p>
    </div>
  );
}