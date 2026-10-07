// 输入区（composer）：工具栏（麦克风开关 + 音量/语速弹层）+ 输入框 + 按钮行（说话分裂按钮 + 发送）。
// 高度由外层分隔条拖拽控制（App.tsx），本组件 flex 填满。快捷键语音输入固定直接发送（识别完即合成），
// 「说话」按钮按 settings.asr_result_mode 分流（转文字 / 直接发送）。

import { useState, useRef, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { VoiceInputButton } from "./VoiceInputButton";
import { VolumeMeter } from "./VolumeMeter";
import { VolumePopover } from "./VolumePopover";
import { MicToggle } from "./MicToggle";
import { useVoiceInputStore } from "../../stores/voiceInputStore";
import { showInfo } from "../common/ConfirmDialog";
import { useSettingsStore } from "../../stores/settingsStore";

interface Props {
  onSend: (text: string) => Promise<void>;
  /** 麦克风未配置设备时点击开关的引导跳转（去设置页） */
  onOpenSettings: () => void;
}

export function InputBox({ onSend, onOpenSettings }: Props) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);

  // 快捷键语音输入会话状态（仅状态条展示；识别结果固定直接发送）
  const phase = useVoiceInputStore((s) => s.phase);
  const recorder = useVoiceInputStore((s) => s.recorder);
  const seconds = useVoiceInputStore((s) => s.seconds);
  const error = useVoiceInputStore((s) => s.error);
  const setVi = useVoiceInputStore((s) => s.set);

  // 快捷键识别结果 → 仅本窗口可见时消费，按所选路径（asr_result_mode）分流：
  // direct 直接合成发送；text 填入输入框（无可见界面时已由 hook 后台直发，不会走到这）
  useEffect(() => {
    const onResult = async (e: Event) => {
      const t = (e as CustomEvent<string>).detail;
      if (!(await getCurrentWindow().isVisible().catch(() => true))) return;
      const mode = useSettingsStore.getState().settings?.asr_result_mode;
      if (mode === "direct") {
        void onSend(t);
      } else {
        setText((prev) => (prev ? prev + t : t));
        inputRef.current?.focus();
      }
    };
    window.addEventListener("voice-input:result", onResult);
    return () => window.removeEventListener("voice-input:result", onResult);
  }, [onSend]);

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
      void showInfo({ title: "发送失败", message: String(e) }); // 可复制全文（含 Key 获取地址）
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 快捷键语音输入状态条（说话按钮的独立会话状态在按钮自身展示） */}
      {(phase !== "idle" || error) && (
        <div
          className={[
            "mx-3 mt-2 flex items-center gap-2 rounded-lg border px-3 py-1.5 text-[11px] animate-fade",
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

      {/* 工具栏 */}
      <div className="flex items-center gap-0.5 px-3 pt-2">
        <MicToggle onOpenSettings={onOpenSettings} />
        <VolumePopover />
      </div>

      {/* 输入框（高度随 composer 拖拽伸展） */}
      <div className="min-h-0 flex-1 px-3 pt-1.5">
        <textarea
          ref={inputRef}
          className="h-full w-full resize-none border-none bg-transparent text-sm leading-relaxed text-[var(--ink-900)] outline-none placeholder:text-[var(--ink-300)]"
          placeholder="输入要朗读的文字，回车发送…"
          value={text}
          autoFocus
          disabled={sending}
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
        />
      </div>

      {/* 按钮行：说话（分裂按钮，模式角标）+ 发送 */}
      <div className="flex items-center justify-end gap-2 px-3 pb-2.5 pt-1">
        <VoiceInputButton
          onResult={(t) => {
            // 转文字模式：识别结果追加到输入框（已有内容时直接拼接，符合中文习惯）
            setText((prev) => (prev ? prev + t : t));
            inputRef.current?.focus();
          }}
          onSend={(t) => {
            // 直接发送模式：识别结果立即合成（与手动发送同一管线）
            void onSend(t);
          }}
        />
        <button
          className="btn-tex rounded-xl px-4 py-2 text-sm font-medium transition-all disabled:cursor-not-allowed active:scale-[0.97]"
          disabled={!text.trim() || sending}
          onClick={() => void send()}
        >
          {sending ? "…" : "发送"}
        </button>
      </div>
    </div>
  );
}

