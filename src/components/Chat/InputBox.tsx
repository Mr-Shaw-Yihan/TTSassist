// 输入区（composer）：工具栏（麦克风开关 + 音量/语速弹层）+ 输入框 + 按钮行（说话分裂按钮 + 发送）。
// 高度由外层分隔条拖拽控制（App.tsx），本组件 flex 填满。快捷键语音输入固定直接发送（识别完即合成），
// 「说话」按钮按 settings.asr_result_mode 分流（转文字 / 直接发送）。

import { useRef, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { VoiceInputButton } from "./VoiceInputButton";
import { VolumeMeter } from "./VolumeMeter";
import { VolumePopover } from "./VolumePopover";
import { MicToggle } from "./MicToggle";
import { useVoiceInputStore } from "../../stores/voiceInputStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useDraftStore } from "../../stores/draftStore";
import { toast } from "../common/Toast";
import { decideSendClick } from "../../utils/sendAction";

interface Props {
  /** 非阻塞发送：立即返回 void（合成在后台跑，状态进气泡），不锁输入框 */
  onSend: (text: string) => void;
  /** 是否有合成在飞（驱动发送键置灰 + 连点打断状态机） */
  busy: boolean;
  /** 置灰期间第二次点：打断当前合成并改发此文本 */
  onInterruptThenSend: (text: string) => void;
  /** 麦克风未配置设备时点击开关的引导跳转（去设置页） */
  onOpenSettings: () => void;
}

export function InputBox({ onSend, busy, onInterruptThenSend, onOpenSettings }: Props) {
  // 草稿放在 store 而不是局部 state：App.tsx 切标签页是条件渲染，本组件会被卸载，
  // 局部 state 归零 → 用户表现为“切个页面刚输入的文字就没了”。setter 兼容函数式更新，
  // 所以下面 setText((prev) => …) 的写法无需改动。
  const text = useDraftStore((s) => s.draft);
  const setText = useDraftStore((s) => s.setDraft);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  // “已武装”：busy 时第一次置灰点击只 arm + 轻提示，3 秒内再点=打断改发。
  const [armed, setArmed] = useState(false);
  const armTimerRef = useRef<number | null>(null);

  function clearArm() {
    if (armTimerRef.current !== null) {
      window.clearTimeout(armTimerRef.current);
      armTimerRef.current = null;
    }
    setArmed(false);
  }
  function armThenTimeout() {
    setArmed(true);
    if (armTimerRef.current !== null) window.clearTimeout(armTimerRef.current);
    armTimerRef.current = window.setTimeout(() => {
      armTimerRef.current = null;
      setArmed(false);
    }, 3000);
  }
  // 合成结束（busy 转假）时自动解除武装，避免下一次发送莫名“一点就打断”
  useEffect(() => {
    if (!busy) clearArm();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy]);
  // 卸载时清定时器
  useEffect(() => {
    return () => {
      if (armTimerRef.current !== null) window.clearTimeout(armTimerRef.current);
    };
  }, []);

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
  // 从其他软件切回主窗时自动聚焦输入框（仅 messages 页挂载本组件时生效）
  useEffect(() => {
    const win = getCurrentWindow();
    let un: (() => void) | null = null;
    (async () => {
      const f = await win.onFocusChanged(({ payload: focused }) => {
        if (focused) window.setTimeout(() => inputRef.current?.focus(), 60);
      });
      un = f;
    })();
    return () => { un?.(); };
  }, []);

  useEffect(() => {
    if (!error) return;
    const t = setTimeout(() => setVi({ error: null }), 6000);
    return () => clearTimeout(t);
  }, [error, setVi]);

  // 发送：非阻塞。handleSend 立即返回，合成失败不再弹框（改由失败气泡重试），
  // 输入框马上清空并继续可用。busy 时走“置灰但仍可点 + 连点两次打断”状态机。
  function handleClick() {
    const t = text.trim();
    const action = decideSendClick({ busy, armed, hasText: !!t });
    if (action === "noop") return;
    if (action === "arm") {
      armThenTimeout();
      void toast("合成进行中，再点一次将打断当前并改发");
      return;
    }
    if (action === "interrupt") {
      clearArm();
      onInterruptThenSend(t);
      setText("");
      inputRef.current?.focus();
      return;
    }
    // send
    clearArm();
    onSend(t);
    setText("");
    inputRef.current?.focus();
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
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              handleClick();
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
          className={[
            "btn-tex rounded-xl px-4 py-2 text-sm font-medium transition-all disabled:cursor-not-allowed active:scale-[0.97]",
            busy ? "opacity-60" : "",
          ].join(" ")}
          disabled={!text.trim()}
          title={busy ? "合成进行中：再点一次打断当前并改发" : "发送"}
          onClick={handleClick}
        >
          {busy && armed ? "打断重发" : "发送"}
        </button>
      </div>
    </div>
  );
}

