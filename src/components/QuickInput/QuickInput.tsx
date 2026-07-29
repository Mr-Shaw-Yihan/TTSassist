// 快捷输入浮窗：极简输入框 + 三态反馈 + 边打字边合成 + 顶部拖拽。
// 大纲 4.7 + 10.x + 阶段 13。

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getAudioUrl } from "../../services/invoke";

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
  const [menu, setMenu] = useState(false);
  const inpRef = useRef<HTMLInputElement | null>(null);

  // 浮窗启动加载 settings.theme 并应用（与主窗独立窗口，data-theme 不共享）
  useEffect(() => {
    (async () => {
      try {
        const s: { theme?: string } = await invoke("get_settings");
        const theme = s.theme === "dark" ? "dark" : "light";
        document.documentElement.setAttribute("data-theme", theme);
      } catch { /* 用默认浅色 */ }
    })();
  }, []);

  // 监听 settings:changed 跟随主窗换肤
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const u = await listen("settings:changed", async () => {
        try {
          const s: { theme?: string } = await invoke("get_settings");
          const theme = s.theme === "dark" ? "dark" : "light";
          document.documentElement.setAttribute("data-theme", theme);
        } catch { /* ignore */ }
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, []);

  // 失去焦点时隐藏浮窗（点击外部自动关闭）
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(({ payload: focused }) => {
        if (!focused) void win.hide();
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

  async function openMain() {
    setMenu(false);
    void (await invoke("show_main_window"));
  }

  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-2xl bg-[var(--paper)] text-[var(--ink-900)] shadow-[0_20px_60px_rgba(26,24,22,0.25)]">
      {/* 顶部条（拖拽区）：拖拽柄 + 标题 + 菜单 */}
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
          <span className="font-display text-xs text-[var(--ink-500)]">语笺</span>
        </div>
        {/* 菜单（独立于拖拽区，可点击） */}
        <div className="relative">
          <button
            onClick={() => setMenu((v) => !v)}
            className="rounded-lg p-1 text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            ⋯
          </button>
          {menu && (
            <>
              <div className="fixed inset-0 z-20" onClick={() => setMenu(false)} />
              <div className="absolute right-0 top-full z-30 mt-1 w-36 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1 text-xs text-[var(--ink-700)] shadow-[0_8px_24px_rgba(26,24,22,0.12)] animate-fade overflow-hidden">
                <button
                  onClick={openMain}
                  className="block w-full px-3 py-2 text-left hover:bg-[var(--amber-200)]/40 hover:text-[var(--ink-900)] transition-colors"
                >
                  打开主界面
                </button>
              </div>
            </>
          )}
        </div>
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
        {status.kind === "converting" && (
          <div className="flex items-center gap-1.5 rounded-lg bg-[var(--amber-200)]/25 px-3 py-1.5 text-[11px] text-[var(--amber-600)] animate-fade">
            <span className="inline-block h-1.5 w-1.5 animate-ping rounded-full bg-[var(--amber-500)]" />
            正在合成语音…
          </div>
        )}
        {status.kind === "success" && (
          <div className="rounded-lg bg-green-500/10 px-3 py-1.5 text-[11px] text-green-600 animate-fade">
            ✓ 已发送并播放
          </div>
        )}
        {status.kind === "error" && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/10 px-3 py-1.5 text-[11px] leading-snug text-[var(--seal)] animate-fade">
            ✗ 合成失败：{status.message}
          </div>
        )}
      </div>
    </div>
  );
}