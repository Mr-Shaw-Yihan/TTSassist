// 语音输入分裂按钮：主按钮点击开始/结束录音，角标下拉选择识别结果去向。
// 交互采用「点按切换」而非「按住说话」：语言障碍用户可能不便长按。
// 两种去向（settings.asr_result_mode，角标可选、持久化）：
//   text   识别后填入输入框，用户自行决定是否发送
//   direct 识别后直接合成发送，跳过改文本（全局快捷键固定走此路径）

import { useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { AudioRecorder } from "../../utils/audioRecorder";
import { VolumeMeter } from "./VolumeMeter";
import { TexIcon } from "../icons/TexIcon";
import { asrTranscribe, listAsrPlugins } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { toast } from "../common/Toast";

interface Props {
  /** 转文字模式：识别文本交给输入框 */
  onResult: (text: string) => void;
  /** 直接发送模式：识别文本立即合成（与手动发送同一管线） */
  onSend: (text: string) => void;
  /** 紧凑形态（浮窗）：字号与内边距收窄 */
  compact?: boolean;
}

type Phase = "idle" | "recording" | "transcribing";

export function VoiceInputButton({ onResult, onSend, compact = false }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [phase, setPhase] = useState<Phase>("idle");
  const [seconds, setSeconds] = useState(0);
  // state 副本供 VolumeMeter 渲染用（ref 变更不触发重渲染）
  const [recorder, setRecorder] = useState<AudioRecorder | null>(null);
  const recorderRef = useRef<AudioRecorder | null>(null);
  const timerRef = useRef<number | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  const mode = settings?.asr_result_mode === "direct" ? "direct" : "text";
  const hotkey = settings?.voice_input_hotkey ?? "";

  // 卸载时兜底释放麦克风 + 关菜单
  useEffect(() => {
    const closeMenu = () => setMenuOpen(false);
    window.addEventListener("mousedown", closeMenu);
    return () => {
      window.removeEventListener("mousedown", closeMenu);
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
          toast("暂无可用的语音识别插件，请先在插件页安装 ASR 插件（如 MiMo ASR）", "err");
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
        toast(`${e}`, "err");
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
        toast("未识别到语音内容，请靠近麦克风说清楚一些再试", "err");
      } else if (mode === "direct") {
        onSend(text.trim());
      } else {
        onResult(text.trim());
      }
    } catch (e) {
      toast(`语音识别失败：${e}`, "err");
    } finally {
      setPhase("idle");
      setSeconds(0);
    }
  }

  function pickMode(m: "text" | "direct") {
    if (m !== mode) void patch("asr_result_mode", m);
    setMenuOpen(false);
  }

  const hotkeyLabel = hotkey ? `快捷键 ${hotkey}` : "未绑定快捷键";
  const title =
    phase === "recording"
      ? "正在录音，点击结束并识别"
      : phase === "transcribing"
        ? "正在识别…"
        : mode === "direct"
          ? `语音输入（${hotkeyLabel} · 直接发送）：点击开始录音，识别后立即合成`
          : `语音输入（${hotkeyLabel} · 转文字）：点击开始录音，识别后填入输入框`;


  return (
    <div className="relative inline-flex" data-speak-split>
      <div className="inline-flex">
        <button
          type="button"
          onClick={toggle}
          title={title}
          disabled={phase === "transcribing"}
          className={[
            "relative inline-flex items-center gap-1.5 border border-[var(--ink-200)] text-[var(--ink-500)] transition-all",
            "rounded-l-xl border-r-0",
            compact ? "px-2.5 py-[6px] text-[11.5px]" : "px-3 py-[7px] text-xs",
            phase === "recording"
              ? "border-red-300 bg-red-50 text-red-600"
              : phase === "transcribing"
                ? "cursor-wait bg-[var(--paper)] text-[var(--ink-300)]"
                : "hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]",
          ].join(" ")}
        >
          {phase === "recording" && (
            <>
              <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-red-500" />
              {recorder && <VolumeMeter recorder={recorder} className="h-1.5 w-8" barClassName="bg-red-500" />}
            </>
          )}
          {phase !== "recording" && <TexIcon name="mic" size={compact ? 12 : 13} />}
          <span>{phase === "recording" ? `${seconds}s · 结束` : phase === "transcribing" ? "识别中…" : "说话"}</span>
        </button>
        <button
          type="button"
          title="选择识别后去向：转文字 / 直接发送"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen((v) => !v);
          }}
          disabled={phase === "transcribing"}
          className={[
            "inline-flex items-center rounded-r-xl border border-l-0 border-[var(--ink-200)] text-[var(--ink-300)] transition-colors",
            compact ? "px-1.5 py-[6px]" : "px-2 py-[7px]",
            phase === "recording" ? "bg-red-50" : "hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]",
          ].join(" ")}
        >
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="m5 9 7 7 7-7" />
          </svg>
        </button>
      </div>

      {/* 模式下拉 */}
      {menuOpen && (
        <div
          className="animate-rise absolute bottom-full right-0 z-40 mb-1.5 w-56 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-1.5 shadow-[0_8px_28px_rgba(26,24,22,0.13)]"
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button
            onClick={() => pickMode("text")}
            className={["flex w-full flex-col items-start gap-0.5 rounded-lg px-2.5 py-2 text-left transition-colors",
              mode === "text" ? "bg-[var(--amber-200)]/40" : "hover:bg-[var(--ink-100)]"].join(" ")}
          >
            <span className="flex w-full items-center gap-1.5 text-[11.5px] font-medium text-[var(--ink-900)]">
              转文字
              {mode === "text" && <span className="ml-auto text-[var(--amber-600)]">✓</span>}
            </span>
            <span className="text-[10px] leading-relaxed text-[var(--ink-300)]">识别后填入输入框，自行决定是否发送</span>
          </button>
          <button
            onClick={() => pickMode("direct")}
            className={["flex w-full flex-col items-start gap-0.5 rounded-lg px-2.5 py-2 text-left transition-colors",
              mode === "direct" ? "bg-[var(--amber-200)]/40" : "hover:bg-[var(--ink-100)]"].join(" ")}
          >
            <span className="flex w-full items-center gap-1.5 text-[11.5px] font-medium text-[var(--ink-900)]">
              直接发送
              {mode === "direct" && <span className="ml-auto text-[var(--amber-600)]">✓</span>}
            </span>
            <span className="text-[10px] leading-relaxed text-[var(--ink-300)]">
              识别后立即合成语音（快捷键语音输入默认此模式）
            </span>
          </button>
        </div>
      )}
    </div>
  );
}
