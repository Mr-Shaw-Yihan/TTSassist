// 输入设备设置（ASR 插件的语音输入配置）：
// 触发快捷键（按住说话） + ASR 插件选择 + 识别语言 + 录音设备 + 端到端测试。
// 设备枚举说明：浏览器只在授予麦克风权限后才返回设备名（label），
// 未授权时显示「授权麦克风」按钮引导用户开启。

import { useState, useEffect, useCallback, useRef } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { listAsrPlugins, asrTranscribe, setVoiceInputHotkey } from "../../services/invoke";
import { AudioRecorder } from "../../utils/audioRecorder";
import { VolumeMeter } from "../Chat/VolumeMeter";
import { HotkeyRecorder } from "./HotkeyRecorder";
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

  // ── 测试录音状态 ──
  type TestPhase = "idle" | "recording" | "transcribing";
  const [testPhase, setTestPhase] = useState<TestPhase>("idle");
  const [testSeconds, setTestSeconds] = useState(0);
  /** 测试结果：null=未测试；ok=识别成功；err=失败 */
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);
  // state 副本供 VolumeMeter 渲染用（ref 变更不触发重渲染）
  const [testRecorder, setTestRecorder] = useState<AudioRecorder | null>(null);
  const testRecorderRef = useRef<AudioRecorder | null>(null);
  const testTimerRef = useRef<number | null>(null);

  // 卸载时兜底释放录音资源
  useEffect(() => {
    return () => {
      testRecorderRef.current?.cancel();
      if (testTimerRef.current) window.clearInterval(testTimerRef.current);
    };
  }, []);

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

  /** 找可用 ASR 插件：优先设置里选的，否则取第一个已加载的 */
  function pickPlugin(): AsrPluginInfo | null {
    if (loadedPlugins.length === 0) return null;
    return loadedPlugins.find((p) => p.id === settings?.asr_plugin) ?? loadedPlugins[0];
  }

  /** 测试：录音→转写→展示结果，验证设备/插件/语言三项配置 */
  async function toggleTest() {
    if (testPhase === "transcribing") return;

    // ── 空闲 → 开始录音 ──
    if (testPhase === "idle") {
      if (!pickPlugin()) {
        setTestResult({ ok: false, text: "无可用识别插件，请先安装 ASR 插件" });
        return;
      }
      setTestResult(null);
      try {
        const recorder = new AudioRecorder();
        await recorder.start(settings?.voice_input_device || undefined);
        testRecorderRef.current = recorder;
        setTestRecorder(recorder);
        setTestPhase("recording");
        setTestSeconds(0);
        testTimerRef.current = window.setInterval(() => setTestSeconds((s) => s + 1), 1000);
      } catch (e) {
        setTestResult({ ok: false, text: `${e}` });
      }
      return;
    }

    // ── 录音中 → 停止并转写 ──
    if (testTimerRef.current) {
      window.clearInterval(testTimerRef.current);
      testTimerRef.current = null;
    }
    const recorder = testRecorderRef.current;
    testRecorderRef.current = null;
    setTestRecorder(null);
    setTestPhase("transcribing");
    try {
      const wav = await (recorder?.stop() ?? Promise.reject(new Error("录音状态异常")));
      const plugin = pickPlugin();
      if (!plugin) throw new Error("无可用识别插件");
      const language = settings?.asr_language || "auto";
      const text = await asrTranscribe(wav, plugin.id, language);
      setTestResult(
        text.trim()
          ? { ok: true, text: text.trim() }
          : { ok: false, text: "未识别到语音内容，请对着麦克风说句话再试" },
      );
    } catch (e) {
      setTestResult({ ok: false, text: `识别失败：${e}` });
    } finally {
      setTestPhase("idle");
    }
  }

  return (
    <div className="space-y-3">
      {/* 触发快捷键（按住说话） */}
      <div>
        <label className="mb-1 block text-[11px] text-[var(--ink-300)]">触发快捷键（按住说话）</label>
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <HotkeyRecorder
              value={settings?.voice_input_hotkey ?? ""}
              onApply={setVoiceInputHotkey}
            />
          </div>
          {settings?.voice_input_hotkey && (
            <button
              onClick={async () => {
                try {
                  await setVoiceInputHotkey("");
                } catch (e) {
                  window.alert(`清除快捷键失败：${e}`);
                }
              }}
              className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-2 text-xs text-[var(--ink-300)] transition-colors hover:border-[var(--seal)] hover:text-[var(--seal)]"
            >
              清除
            </button>
          )}
        </div>
        <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
          按下快捷键响叮咚声并开始录音，松开快捷键再响一声并自动识别，结果填入输入框。
        </p>
      </div>

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
              {requesting ? "授权中…" : "授权麦克风"}
            </button>
          </div>
        )}
        <p className="mt-1 text-[10px] text-[var(--ink-300)]">
          所选设备仅用于语音输入录音；设备拔插后会自动回退到系统默认麦克风。
        </p>
      </div>

      {/* 测试：用当前配置录一句并识别，验证设备/插件/语言是否就绪 */}
      <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5">
        <label className="mb-1.5 block text-[11px] text-[var(--ink-300)]">测试</label>
        <div className="flex items-center gap-2">
          <button
            onClick={toggleTest}
            disabled={testPhase === "transcribing"}
            className={[
              "flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
              testPhase === "recording"
                ? "border-red-300 bg-red-50 text-red-600 hover:bg-red-100"
                : "border-[var(--ink-200)] bg-[var(--paper)] text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]",
            ].join(" ")}
          >
            {testPhase === "recording" && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-red-500" />}
            {testPhase === "recording"
              ? `${testSeconds}s 点击停止`
              : testPhase === "transcribing"
                ? "识别中…"
                : "开始测试"}
          </button>
          <span className="text-[10px] text-[var(--ink-300)]">
            {testPhase === "recording" ? "对着麦克风说句话" : "用当前配置录一句并识别"}
          </span>
        </div>
        {testPhase === "recording" && testRecorder && (
          <VolumeMeter recorder={testRecorder} className="mt-2 h-2 w-full" barClassName="bg-red-500" />
        )}
        {testResult && (
          <div
            className={[
              "mt-2 rounded-md border px-2.5 py-2 text-[11px] leading-relaxed",
              testResult.ok
                ? "border-green-200 bg-green-50 text-green-700"
                : "border-red-200 bg-red-50 text-red-600",
            ].join(" ")}
          >
            {testResult.ok ? (
              <>
                <span className="font-medium">✓ 识别成功：</span>
                <span className="select-all">“{testResult.text}”</span>
              </>
            ) : (
              <>
                <span className="font-medium">✗ </span>
                {testResult.text}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
