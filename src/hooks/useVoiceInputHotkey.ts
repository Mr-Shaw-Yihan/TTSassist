// 语音输入全局快捷键会话：按下开始录音（叮提示音），松开结束（咚提示音）
// → ASR 转写 → 派发 voice-input:result 窗口事件。
//
// 主窗与浮窗各自挂载本 hook；仅「可见」窗口处理事件（两窗显隐互斥），
// 避免两个窗口同时抢麦克风。

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AudioRecorder } from "../utils/audioRecorder";
import { asrTranscribe, listAsrPlugins } from "../services/invoke";
import { useSettingsStore } from "../stores/settingsStore";
import { useVoiceInputStore, emitVoiceInputResult } from "../stores/voiceInputStore";
import { playStartChime, playEndChime } from "../utils/chime";

export function useVoiceInputHotkey() {
  const recorderRef = useRef<AudioRecorder | null>(null);
  const timerRef = useRef<number | null>(null);
  /** startRecording 正在异步启动（申请麦克风等），防止重复进入与竞态判定 */
  const startingRef = useRef(false);
  /** 录音还没启动完成就已松开快捷键 → 启动完成后立即静默取消 */
  const releasedEarlyRef = useRef(false);
  /** 已加载 ASR 插件 id 缓存（挂载时预查，省掉按下时的 IPC 往返） */
  const pluginIdCacheRef = useRef<string | null>(null);

  useEffect(() => {
    let unPressed: (() => void) | null = null;
    let unReleased: (() => void) | null = null;
    let cancelled = false;

    const store = useVoiceInputStore;

    /** 挑可用 ASR 插件：优先设置里选的，否则第一个已加载的 */
    async function pickPluginId(): Promise<string | null> {
      try {
        const plugins = await listAsrPlugins();
        const loaded = plugins.filter((p) => p.loaded);
        const preferred = useSettingsStore.getState().settings?.asr_plugin;
        return loaded.find((p) => p.id === preferred)?.id ?? loaded[0]?.id ?? null;
      } catch {
        return null;
      }
    }

    async function startRecording() {
      // 上一次按下还没启动完（或已在录音/转写）→ 忽略本次按下
      if (startingRef.current || store.getState().phase !== "idle") return;
      // 仅可见窗口处理（主窗/浮窗互斥，防止双录）
      const visible = await getCurrentWindow().isVisible().catch(() => true);
      if (!visible) return;
      if (store.getState().phase !== "idle") return;
      startingRef.current = true;
      releasedEarlyRef.current = false;
      const settings = useSettingsStore.getState().settings;

      let pluginId = pluginIdCacheRef.current;
      if (!pluginId) {
        pluginId = await pickPluginId();
        pluginIdCacheRef.current = pluginId;
      }
      if (!pluginId) {
        startingRef.current = false;
        store.getState().set({ error: "暂无可用的语音识别插件，请先安装 ASR 插件" });
        return;
      }

      // 提示音提前到麦克风启动前播放：按下即有反馈，不等 getUserMedia
      playStartChime();
      try {
        const recorder = new AudioRecorder();
        await recorder.start(settings?.voice_input_device || undefined);
        if (releasedEarlyRef.current) {
          // 启动期间已松开：不产生录音，静默取消（修复"松手后仍在录"）
          recorder.cancel();
          startingRef.current = false;
          return;
        }
        recorderRef.current = recorder;
        store.getState().set({ phase: "recording", recorder, seconds: 0, error: null });
        timerRef.current = window.setInterval(() => {
          store.getState().set({ seconds: store.getState().seconds + 1 });
        }, 1000);
        startingRef.current = false;
      } catch (e) {
        startingRef.current = false;
        store.getState().set({ error: `${e}` });
      }
    }

    async function stopRecording() {
      // 录音还没启动完成就松开 → 记标记，启动完成后自行取消，这里直接返回
      if (startingRef.current) {
        releasedEarlyRef.current = true;
        return;
      }
      if (store.getState().phase !== "recording") return;
      if (timerRef.current) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
      const recorder = recorderRef.current;
      recorderRef.current = null;
      store.getState().set({ phase: "transcribing", recorder: null });
      playEndChime();
      try {
        const wav = await (recorder?.stop() ?? Promise.reject(new Error("录音状态异常")));
        const settings = useSettingsStore.getState().settings;
        const pluginId = await pickPluginId();
        if (!pluginId) throw new Error("暂无可用的语音识别插件");
        const text = await asrTranscribe(wav, pluginId, settings?.asr_language || "auto");
        if (text.trim()) {
          emitVoiceInputResult(text.trim());
          store.getState().set({ phase: "idle", error: null });
        } else {
          store.getState().set({ phase: "idle", error: "未识别到语音内容，请对着麦克风说句话再试" });
        }
      } catch (e) {
        store.getState().set({ phase: "idle", error: `语音识别失败：${e}` });
      }
    }

    (async () => {
      // 预查 ASR 插件（首次按下不用等 IPC）
      pluginIdCacheRef.current = await pickPluginId();
      const a = await listen("voice-input:pressed", () => void startRecording());
      const b = await listen("voice-input:released", () => void stopRecording());
      if (cancelled) {
        a();
        b();
      } else {
        unPressed = a;
        unReleased = b;
      }
    })();

    return () => {
      cancelled = true;
      unPressed?.();
      unReleased?.();
      if (timerRef.current) window.clearInterval(timerRef.current);
      recorderRef.current?.cancel();
      recorderRef.current = null;
    };
  }, []);
}
