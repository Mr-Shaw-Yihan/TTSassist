// 输入框组件：回车/按钮发送，调 generate_tts；集成语音输入（ASR）
// 快捷键按住说话的录音状态条也展示在此（会话由 useVoiceInputHotkey 驱动）。
// 大纲 4.2

import { useState, useRef, useEffect } from "react";
import { VoiceInputButton } from "./VoiceInputButton";
import { VolumeMeter } from "./VolumeMeter";
import { useVoiceInputStore } from "../../stores/voiceInputStore";

interface Props {
  onSend: (text: string) => Promise<void>;
}

export function InputBox({ onSend }: Props) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // 快捷键语音输入会话状态
  const phase = useVoiceInputStore((s) => s.phase);
  const recorder = useVoiceInputStore((s) => s.recorder);
  const seconds = useVoiceInputStore((s) => s.seconds);
  const error = useVoiceInputStore((s) => s.error);
  const setVi = useVoiceInputStore((s) => s.set);

  // 快捷键识别结果 → 填入输入框
  useEffect(() => {
    const onResult = (e: Event) => {
      const t = (e as CustomEvent<string>).detail;
      setText((prev) => (prev ? prev + t : t));
      inputRef.current?.focus();
    };
    window.addEventListener("voice-input:result", onResult);
    return () => window.removeEventListener("voice-input:result", onResult);
  }, []);

  // 错误提示 6 秒后自动消失
  useEffect(() => {
    if (!error) return;
    const t = setTimeout(() => setVi({ error: null }), 6000);
    return () => clearTimeout(t);
  }, [error, setVi]);

  async function send() {
    const t = text.trim();
    if (!t || sending) return;
    setSending(true);
    try {
      await onSend(t);
      setText("");
      inputRef.current?.focus();
    } catch (e) {
      window.alert(`发送失败：${e}`);
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="space-y-2">
      {/* 快捷键语音输入状态条 */}
      {(phase !== "idle" || error) && (
        <div
          className={[
            "flex items-center gap-2 rounded-lg border px-3 py-1.5 text-[11px] animate-fade",
            error
              ? "border-[var(--seal)]/30 bg-[var(--seal)]/5 text-[var(--seal)]"
              : "border-red-200 bg-red-50 text-red-600",
          ].join(" ")}
        >
          {phase === "recording" && (
            <>
              <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-red-500" />
              <span>录音中，松开快捷键结束 · {seconds}s</span>
              {recorder && <VolumeMeter recorder={recorder} className="h-1.5 flex-1" barClassName="bg-red-500" />}
            </>
          )}
          {phase === "transcribing" && <span className="animate-pulse">正在识别，请稍候…</span>}
          {phase === "idle" && error && <span>✗ {error}</span>}
        </div>
      )}

      <div className="flex gap-2">
        <VoiceInputButton
          onResult={(t) => {
            // 识别结果追加到输入框（已有内容时直接拼接，符合中文习惯）
            setText((prev) => (prev ? prev + t : t));
            inputRef.current?.focus();
          }}
        />
        <input
          ref={inputRef}
          className="flex-1 rounded-xl border border-[var(--ink-200)] bg-[var(--paper)] px-3.5 py-2.5 text-sm text-[var(--ink-900)] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
          placeholder="输入要朗读的文字，回车发送…"
          value={text}
          autoFocus
          disabled={sending}
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
        />
        <button
          className="rounded-xl bg-[var(--ink-900)] px-4 py-2.5 text-sm font-medium text-[var(--paper)] transition-all hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:bg-[var(--ink-200)] disabled:text-[var(--ink-300)] active:scale-[0.97]"
          disabled={!text.trim() || sending}
          onClick={send}
        >
          {sending ? "…" : "发"}
        </button>
      </div>
    </div>
  );
}