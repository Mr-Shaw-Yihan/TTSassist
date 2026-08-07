// 语音输入（快捷键按住说话）会话状态：同一窗口内跨组件共享。
// 会话由 hooks/useVoiceInputHotkey 驱动；InputBox/QuickInput 读状态展示录音条、
// 监听 voice-input:result 窗口事件接收识别文本。

import { create } from "zustand";
import type { AudioRecorder } from "../utils/audioRecorder";

export type VoiceInputPhase = "idle" | "recording" | "transcribing";

interface VoiceInputState {
  phase: VoiceInputPhase;
  /** 录音中的录音器（供音量条读取实时音量） */
  recorder: AudioRecorder | null;
  /** 录音已持续秒数 */
  seconds: number;
  /** 最近一次错误提示（null=无） */
  error: string | null;
  set: (partial: Partial<Omit<VoiceInputState, "set">>) => void;
}

export const useVoiceInputStore = create<VoiceInputState>((set) => ({
  phase: "idle",
  recorder: null,
  seconds: 0,
  error: null,
  set: (partial) => set(partial),
}));

/** 派发识别结果：窗口内有输入框的组件监听后填入 */
export function emitVoiceInputResult(text: string) {
  window.dispatchEvent(new CustomEvent<string>("voice-input:result", { detail: text }));
}
