// 语音输入按钮：点击开始录音 → 再点击结束 → ASR 转写 → 文本填入输入框。
//
// 交互采用「点按切换」而非「按住说话」：语言障碍用户可能不便长按，
// 单击开始/单击结束对运动控制要求最低。

import { useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { AudioRecorder } from "../../utils/audioRecorder";
import { VolumeMeter } from "./VolumeMeter";
import { MicIcon } from "../icons/MicIcon";
import { asrTranscribe, listAsrPlugins } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";

interface Props {
  /** 转写完成后把文本交给输入框 */
  onResult: (text: string) => void;
}

type Phase = "idle" | "recording" | "transcribing";

export function VoiceInputButton({ onResult }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const [phase, setPhase] = useState<Phase>("idle");
  const [seconds, setSeconds] = useState(0);
  // state 副本供 VolumeMeter 渲染用（ref 变更不触发重渲染）
  const [recorder, setRecorder] = useState<AudioRecorder | null>(null);
  const recorderRef = useRef<AudioRecorder | null>(null);
  const timerRef = useRef<number | null>(null);

  // 卸载时兜底释放麦克风
  useEffect(() => {
    return () => {
      recorderRef.current?.cancel();
      if (timerRef.current) window.clearInterval(timerRef.current);
    };
  }, []);

  function startTimer() {
    setSeconds(0);
    timerRef.current = window.setInterval(() => setSeconds((s) => s + 1), 1000);
  }

  function stopTimer() {
    if (timerRef.current) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }

  /** 找可用的 ASR 插件：优先设置里选的，否则取第一个已加载的 */
  async function pickPlugin(): Promise<{ id: string; language: string } | null> {
    const plugins = await listAsrPlugins();
    const loaded = plugins.filter((p) => p.loaded);
    if (loaded.length === 0) return null;
    const preferred = settings?.asr_plugin;
    const chosen = loaded.find((p) => p.id === preferred) ?? loaded[0];
    const language = settings?.asr_language || "auto";
    return { id: chosen.id, language };
  }

  async function toggle() {
    if (phase === "transcribing") return;

    // ── 空闲 → 开始录音 ──
    if (phase === "idle") {
      try {
        const target = await pickPlugin();
        if (!target) {
          window.alert("暂无可用的语音识别插件，请先在插件页安装 ASR 插件（如 MiMo ASR）");
          return;
        }
        const recorder = new AudioRecorder();
        await recorder.start(settings?.voice_input_device || undefined);
        recorderRef.current = recorder;
        setRecorder(recorder);
        setPhase("recording");
        startTimer();
        void emit("va:asr:start").catch(() => {});
      } catch (e) {
        window.alert(`${e}`);
      }
      return;
    }

    // ── 录音中 → 停止并转写 ──
    stopTimer();
    const recorder = recorderRef.current;
    recorderRef.current = null;
    setRecorder(null);
    setPhase("transcribing");
    void emit("va:asr:end").catch(() => {});
    try {
      const wav = await (recorder?.stop() ?? Promise.reject(new Error("录音状态异常")));
      const target = await pickPlugin();
      if (!target) throw new Error("ASR 插件不可用");
      const text = await asrTranscribe(wav, target.id, target.language);
      if (!text.trim()) {
        window.alert("未识别到语音内容，请靠近麦克风说清楚一些再试");
      } else {
        onResult(text.trim());
      }
    } catch (e) {
      window.alert(`语音识别失败：${e}`);
    } finally {
      setPhase("idle");
      setSeconds(0);
    }
  }

  const title =
    phase === "recording"
      ? "正在录音，点击结束并识别"
      : phase === "transcribing"
        ? "正在识别…"
        : "语音输入：点击开始录音";

  return (
    <button
      type="button"
      onClick={toggle}
      title={title}
      disabled={phase === "transcribing"}
      className={[
        "relative flex shrink-0 items-center gap-1 rounded-xl border px-3 py-2.5 text-sm font-medium transition-all",
        phase === "recording"
          ? "border-red-300 bg-red-50 text-red-600"
          : phase === "transcribing"
            ? "cursor-wait border-[var(--ink-200)] bg-[var(--paper)] text-[var(--ink-300)]"
            : "border-[var(--ink-200)] bg-[var(--paper)] text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] active:scale-[0.97]",
      ].join(" ")}
    >
      {phase === "recording" && (
        <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" />
      )}
      {phase === "recording" && recorder && (
        <VolumeMeter recorder={recorder} className="h-1.5 w-9" barClassName="bg-red-500" />
      )}
      <span className="flex items-center">{phase === "transcribing" ? "…" : <MicIcon size={14} />}</span>
      <span className="text-xs">
        {phase === "recording" ? `${seconds}s` : phase === "transcribing" ? "识别中" : "说话"}
      </span>
    </button>
  );
}
