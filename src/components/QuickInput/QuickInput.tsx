// 快捷输入浮窗：极简输入框 + 三态反馈 + 边打字边合成 + 顶部拖拽。
// 大纲 4.7 + 10.x + 阶段 13。

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getAudioUrl, getSettings } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { useVoiceInputHotkey } from "../../hooks/useVoiceInputHotkey";
import { useVoiceInputStore } from "../../stores/voiceInputStore";
import { VolumeMeter } from "../Chat/VolumeMeter";
import { MicIcon } from "../icons/MicIcon";

/** 发送/合成的三态反馈 */
type Status =
  | { kind: "idle" }
  | { kind: "converting" }
  | { kind: "success" }
  | { kind: "error"; message: string };

export function QuickInput() {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const inpRef = useRef<HTMLInputElement | null>(null);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // 语音输入全局快捷键会话（按住说话）：浮窗是游戏内主场景，必须支持
  useVoiceInputHotkey();
  const viPhase = useVoiceInputStore((s) => s.phase);
  const viRecorder = useVoiceInputStore((s) => s.recorder);
  const viSeconds = useVoiceInputStore((s) => s.seconds);
  const viError = useVoiceInputStore((s) => s.error);

  // 快捷键识别结果 → 填入浮窗输入框；错误提示 6 秒后自动消失
  useEffect(() => {
    const onResult = (e: Event) => {
      const t = (e as CustomEvent<string>).detail;
      setText((prev) => (prev ? prev + t : t));
      inpRef.current?.focus();
    };
    window.addEventListener("voice-input:result", onResult);
    return () => window.removeEventListener("voice-input:result", onResult);
  }, []);
  useEffect(() => {
    if (!viError) return;
    const t = setTimeout(() => useVoiceInputStore.getState().set({ error: null }), 6000);
    return () => clearTimeout(t);
  }, [viError]);

  // 应用主题
  function applyTheme(theme?: string) {
    document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
  }

  // 浮窗启动：加载 settings 到 store（供 MicToggle 用）并应用主题
  useEffect(() => {
    (async () => {
      try {
        const s = await getSettings();
        setSettings(s);
        applyTheme(s.theme);
      } catch { /* 用默认 */ }
    })();
  }, [setSettings]);

  // 监听 settings:changed：同步 store + 换肤（跟随主窗设置变化）
  useTauriListen("settings:changed", async () => {
    try {
      const s = await getSettings();
      setSettings(s);
      applyTheme(s.theme);
    } catch { /* ignore */ }
  }, [setSettings]);

  // 失去焦点时隐藏浮窗（点击外部自动关闭）；录音中不隐藏，避免会话被打断
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(({ payload: focused }) => {
        if (focused) return;
        const phase = useVoiceInputStore.getState().phase;
        if (phase !== "idle") return;
        void win.hide();
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, []);

  // 每次显示时自动聚焦输入框
  useEffect(() => {
    const t = setTimeout(() => inpRef.current?.focus(), 80);
    return () => clearTimeout(t);
  }, []);

  async function send() {
    const t = text.trim();
    // 合成期间仍可打字，但不重复发送
    if (!t || sending) return;
    setText(""); // 立即清空输入框，合成期间可直接输入下一句
    setSending(true);
    setStatus({ kind: "converting" });
    try {
      const msg: { audio_path: string } = await invoke("generate_tts", { text: t });
      // 播放语音（麦克风由后端 generate_tts 按全局开关自动处理）
      try {
        const s: { playback_volume?: number; playback_rate?: number } = await invoke("get_settings");
        const url = await getAudioUrl(msg.audio_path);
        const a = new Audio(url);
        a.volume = s.playback_volume ?? 0.8;
        a.playbackRate = s.playback_rate ?? 1.0;
        void a.play();
      } catch { /* 播放失败不影响发送 */ }
      setStatus({ kind: "success" });
      // 1.2s 后淡出成功提示（若期间没有新状态覆盖）
      setTimeout(() => {
        setStatus((cur) => (cur.kind === "success" ? { kind: "idle" } : cur));
      }, 1200);
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    } finally {
      setSending(false);
    }
  }

  // 打开主界面并关闭浮窗（show_main_window 后端会同时隐藏浮窗）
  async function openMainAndClose() {
    void (await invoke("show_main_window"));
  }

  // 语音输入按钮（点击切换）：emit 与全局快捷键相同的事件，复用快捷键会话链路
  const viHotkey = useSettingsStore((s) => s.settings?.voice_input_hotkey);
  function toggleVoiceInput() {
    if (viPhase === "recording") {
      void emit("voice-input:released");
    } else if (viPhase === "idle") {
      void emit("voice-input:pressed");
    }
    // 识别中（transcribing）点击无效，按钮已禁用
  }

  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-2xl bg-[var(--paper)] text-[var(--ink-900)] shadow-[0_20px_60px_rgba(26,24,22,0.25)]">
      {/* 顶部条：拖拽区 + 语音输入 + 打开主界面（发送到麦克风开关在主界面「其他」面板） */}
      <div className="flex select-none items-center px-3 pt-2 pb-1">
        {/* 拖拽区（⠿ + 标题 + 空白）：按住左键拖动移动浮窗 */}
        <div
          data-tauri-drag-region
          onMouseDown={(e) => {
            if (e.button === 0) void getCurrentWindow().startDragging();
          }}
          className="flex flex-1 cursor-move items-center gap-2"
        >
          <span className="text-[var(--ink-300)]">⠿</span>
          <span className="font-display text-xs text-[var(--ink-500)]">电子声带</span>
        </div>
        {/* 语音输入按钮（点击切换录音；快捷键见 title 提示） */}
        <button
          onClick={toggleVoiceInput}
          disabled={viPhase === "transcribing"}
          title={
            viPhase === "recording"
              ? "点击结束录音并识别"
              : viHotkey
                ? `语音输入（快捷键 ${viHotkey}，按住说话）`
                : "语音输入（可在设置-语音输入中绑定快捷键）"
          }
          className={[
            "rounded-lg p-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-50",
            viPhase === "recording"
              ? "animate-pulse bg-red-500/15 text-red-500"
              : "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
          ].join(" ")}
        >
          <MicIcon size={15} />
        </button>
        {/* 打开主界面并关闭浮窗 */}
        <button
          onClick={openMainAndClose}
          title="打开主界面"
          className="ml-0.5 flex rounded-lg p-1.5 text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <rect x="4" y="4" width="16" height="16" rx="2" />
            <path d="M4 9h16" />
          </svg>
        </button>
      </div>

      {/* 输入行（合成期间仍可打字） */}
      <div className="flex items-center gap-1.5 px-3 py-2">
        <input
          ref={inpRef}
          className="flex-1 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm text-[var(--ink-900)] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
          placeholder="输入文字，回车发送…"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
        />
        <button
          onClick={send}
          disabled={!text.trim() || sending}
          className="rounded-xl bg-[var(--ink-900)] px-3.5 py-2 text-sm font-medium text-[var(--paper)] transition-all hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:bg-[var(--ink-200)] disabled:text-[var(--ink-300)] active:scale-[0.97]"
        >
          {sending ? "…" : "发"}
        </button>
      </div>

      {/* 状态条（三态反馈，常驻占位避免跳动，内容按需显示） */}
      <div className="px-3 pb-2.5">
        {viPhase === "recording" && (
          <div className="flex items-center gap-1.5 rounded-lg border border-red-200 bg-red-50 px-3 py-1.5 text-[11px] text-red-600 animate-fade">
            <span className="inline-block h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-red-500" />
            <span className="shrink-0">录音中 · {viSeconds}s</span>
            {viRecorder && <VolumeMeter recorder={viRecorder} className="h-1.5 flex-1" barClassName="bg-red-500" />}
          </div>
        )}
        {viPhase === "transcribing" && (
          <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-1.5 text-[11px] text-red-600 animate-fade">
            <span className="animate-pulse">正在识别，请稍候…</span>
          </div>
        )}
        {viPhase === "idle" && viError && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/10 px-3 py-1.5 text-[11px] leading-snug text-[var(--seal)] animate-fade">
            ✗ {viError}
          </div>
        )}
        {viPhase === "idle" && !viError && status.kind === "converting" && (
          <div className="flex items-center gap-1.5 rounded-lg bg-[var(--amber-200)]/25 px-3 py-1.5 text-[11px] text-[var(--amber-600)] animate-fade">
            <span className="inline-block h-1.5 w-1.5 animate-ping rounded-full bg-[var(--amber-500)]" />
            正在合成语音…
          </div>
        )}
        {viPhase === "idle" && !viError && status.kind === "success" && (
          <div className="rounded-lg bg-green-500/10 px-3 py-1.5 text-[11px] text-green-600 animate-fade">
            ✓ 已发送并播放
          </div>
        )}
        {viPhase === "idle" && !viError && status.kind === "error" && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/10 px-3 py-1.5 text-[11px] leading-snug text-[var(--seal)] animate-fade">
            ✗ 合成失败：{status.message}
          </div>
        )}
      </div>
    </div>
  );
}