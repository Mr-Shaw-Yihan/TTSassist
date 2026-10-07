// 快捷输入浮窗：仿主界面「工具栏 + 输入框」结构（工具栏兼拖拽区）+ 三态反馈。
// 顶部条取消：语音输入由右下角「说话」分裂按钮承担（模式跟随主界面），
// 麦克风发送开关与音量/语速从主窗「其他」移入工具栏；右上角保留「打开主界面」。
// 直播伴侣形态：窗口永久挂 WS_EX_NOACTIVATE（永不激活、游戏保前台），
// 键盘路由靠点击输入框后后端 SetFocus webview 子窗口建立。
// 全局快捷键语音输入固定直接发送（识别完即合成，不进输入框）。

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { cursorPosition, getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";
import { getAudioUrl, getSettings, generateTTS } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { useVoiceInputHotkey } from "../../hooks/useVoiceInputHotkey";
import { useVoiceInputStore } from "../../stores/voiceInputStore";
import { VolumeMeter } from "../Chat/VolumeMeter";
import { MicToggle } from "../Chat/MicToggle";
import { VolumePopover } from "../Chat/VolumePopover";
import { VoiceInputButton } from "../Chat/VoiceInputButton";
import { TexDefs } from "../icons/TexIcon";
import { ToastHost } from "../common/Toast";

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
  // 直播伴侣形态：窗口永久挂 WS_EX_NOACTIVATE（永不激活、游戏保前台），
  // 键盘路由靠点击输入框后后端 SetFocus webview 子窗口建立。
  // typing = 键盘路由已建立（驱动 placeholder 文案）。
  const [typing, setTyping] = useState(false);
  const inpRef = useRef<HTMLTextAreaElement | null>(null);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // 语音输入全局快捷键会话（按住说话）：浮窗是游戏内主场景，必须支持。
  // 快捷键识别结果固定直接发送（不进输入框）。
  useVoiceInputHotkey();
  const viPhase = useVoiceInputStore((s) => s.phase);
  const viRecorder = useVoiceInputStore((s) => s.recorder);
  const viSeconds = useVoiceInputStore((s) => s.seconds);
  const viError = useVoiceInputStore((s) => s.error);

  // 快捷键识别结果 → 仅本窗口可见时消费，按所选路径（asr_result_mode）分流：
  // direct 直接合成发送；text 填入输入框（无可见界面时已由 hook 后台直发，不会走到这）
  useEffect(() => {
    const onResult = async (e: Event) => {
      const t = (e as CustomEvent<string>).detail;
      if (!(await getCurrentWindow().isVisible().catch(() => true))) return;
      const mode = useSettingsStore.getState().settings?.asr_result_mode;
      if (mode === "direct") {
        void sendText(t);
      } else {
        setText((prev) => (prev ? prev + t : t));
        inpRef.current?.focus();
        takeKeyboardFocus(); // 识别完回到打字态：重建键盘路由
      }
    };
    window.addEventListener("voice-input:result", onResult);
    return () => window.removeEventListener("voice-input:result", onResult);
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  // 浮窗启动：加载 settings 到 store（供 MicToggle/VolumePopover 用）并应用主题
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

  // 建立键盘路由：后端 SetFocus webview 子窗口（不激活窗口、前台仍是游戏）。
  // 点击输入框时调用；焦点会被游戏内其它点击夺走，再点输入框即重建。
  function takeKeyboardFocus() {
    void invoke("focus_quick_input_content").catch(() => {});
    setTyping(true);
  }

  // ESC 关闭浮窗：键盘路由建立后生效；未点击浮窗时用快捷键/悬浮球收起。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void getCurrentWindow().hide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 每次显示时自动聚焦输入框（挂载时一次）
  useEffect(() => {
    const t = setTimeout(() => inpRef.current?.focus(), 80);
    return () => clearTimeout(t);
  }, []);

  // 窗口常驻挂载（隐藏不销毁）；NOACTIVATE 形态下焦点事件不再发生，
  // 此监听仅作兼容保留（意外获焦时补聚焦输入框）
  useEffect(() => {
    const win = getCurrentWindow();
    let un: (() => void) | null = null;
    (async () => {
      un = await win.onFocusChanged(({ payload }) => {
        if (!payload) return;
        window.setTimeout(() => inpRef.current?.focus(), 50);
      });
    })();
    return () => { un?.(); };
  }, []);

  /** 发送指定文本（合成 → 自动播放 → 状态反馈）。直接发送模式与手动发送共用。 */
  async function sendText(raw: string) {
    const t = raw.trim();
    // 合成期间仍可打字，但不重复发送
    if (!t || sending) return;
    setText(""); // 立即清空输入框，合成期间可直接输入下一句
    setSending(true);
    setStatus({ kind: "converting" });
    try {
      const { message: msg, hostPlayed } = await generateTTS(t);
      // 播放语音（麦克风由后端 generate_tts 按全局开关自动处理）；
      // 宿主已流式播放（扬声器）则跳过整段播放，避免双播
      if (!hostPlayed) {
        try {
          const s: { playback_volume?: number; playback_rate?: number } = await invoke("get_settings");
          const url = await getAudioUrl(msg.audio_path);
          const a = new Audio(url);
          a.volume = s.playback_volume ?? 0.8;
          a.playbackRate = s.playback_rate ?? 1.0;
          a.addEventListener("ended", () => {
            void emit("va:play:stop").catch(() => {});
            void emit("playback:stopped").catch(() => {});
          });
          void emit("va:play:start").catch(() => {});
          // 播放态上报（后端聚合为通用播放状态，供宿主能力桥订阅方感知）
          void emit("playback:started", msg.audio_path).catch(() => {});
          void a.play();
        } catch { /* 播放失败不影响发送 */ }
      }
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

  // 手动拖拽跟随鼠标：不用 startDragging（系统标题栏拖拽路径会触发 Windows 贴靠吸附，
  // 表现为"跳到固定位置"而非跟随鼠标）。记录光标与窗口左上角的偏移，mousemove 实时 setPosition。
  async function onToolbarDragStart(e: React.MouseEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const win = getCurrentWindow();
    try {
      const [cursor, winPos] = await Promise.all([cursorPosition(), win.outerPosition()]);
      const offX = winPos.x - cursor.x;
      const offY = winPos.y - cursor.y;
      const onMove = async () => {
        const c = await cursorPosition();
        await win.setPosition(new PhysicalPosition(c.x + offX, c.y + offY));
      };
      const onMoveSafe = () => { onMove().catch(() => {}); };
      const up = () => {
        window.removeEventListener("mousemove", onMoveSafe);
        window.removeEventListener("mouseup", up);
      };
      window.addEventListener("mousemove", onMoveSafe);
      window.addEventListener("mouseup", up);
    } catch {
      /* 取光标/窗口位置失败则本次不拖动 */
    }
  }

  return (
    <div
      className="flex h-screen flex-col overflow-hidden rounded-2xl bg-[var(--paper)] text-[var(--ink-900)] shadow-[0_20px_60px_rgba(26,24,22,0.25)]"
    >
      {/* 纹理图标符号库：浮窗是独立 WebView，必须自带（主窗挂的 TexDefs 跨窗口不可见） */}
      <TexDefs />
      <ToastHost />
      {/* 工具栏（兼拖拽区）：麦克风开关 / 音量语速 · 空白处按住拖动浮窗 · 打开主界面 */}
      <div className="flex select-none items-center gap-0.5 px-2 pt-2">
        <MicToggle onOpenSettings={openMainAndClose} />
        <VolumePopover direction="down" />
        {/* 拖拽区：按住左键移动浮窗（手动跟随鼠标，不走系统拖拽以免触发贴靠吸附） */}
        <div
          onMouseDown={(e) => void onToolbarDragStart(e)}
          title="按住拖动，移动浮窗位置"
          className="h-8 min-w-0 flex-1 cursor-move"
        />
        {/* 打开主界面并关闭浮窗（ESC 也可关闭浮窗） */}
        <button
          onClick={openMainAndClose}
          title="打开主界面 · ESC 关闭浮窗"
          className="flex h-8 w-8 items-center justify-center rounded-[9px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <rect x="4" y="4" width="16" height="16" rx="2" />
            <path d="M4 9h16" />
          </svg>
        </button>
      </div>

      {/* 无边框输入区（撑满窗口剩余空间，合成期间仍可打字） */}
      <div className="flex min-h-0 flex-1 px-3.5 pt-2.5">
        <textarea
          ref={inpRef}
          className="h-full w-full resize-none border-none bg-transparent text-sm leading-relaxed text-[var(--ink-900)] outline-none placeholder:text-[var(--ink-300)]"
          placeholder={typing ? "输入文字，回车发送…" : "点击输入框开始输入…"}
          value={text}
          onPointerDown={takeKeyboardFocus}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void sendText(text);
            }
          }}
        />
      </div>

      {/* 按钮行：说话（分裂按钮，模式跟随主界面）+ 发送 */}
      <div className="flex items-center justify-end gap-2 px-3 pb-2">
        <VoiceInputButton
          compact
          onResult={(t) => {
            // 转文字模式：识别结果追加到输入框
            setText((prev) => (prev ? prev + t : t));
            inpRef.current?.focus();
            takeKeyboardFocus(); // 识别完回到打字态：重建键盘路由
          }}
          onSend={(t) => {
            // 直接发送模式：识别结果立即合成
            void sendText(t);
          }}
        />
        <button
          onClick={() => void sendText(text)}
          disabled={!text.trim() || sending}
          className="btn-tex rounded-xl px-3.5 py-[7px] text-[12.5px] font-medium transition-all disabled:cursor-not-allowed active:scale-[0.97]"
        >
          {sending ? "…" : "发送"}
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
