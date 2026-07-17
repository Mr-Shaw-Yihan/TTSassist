// 快捷输入浮窗：极简输入框，发送后"已发送"提示，点击外部自动隐藏。
// 大纲 4.7 + 10.x

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getAudioUrl } from "../../services/invoke";

export function QuickInput() {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);
  const [menu, setMenu] = useState(false);
  const inpRef = useRef<HTMLInputElement | null>(null);

  // 失去焦点时隐藏浮窗（点击外部自动关闭）
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(({ payload: focused }) => {
        if (!focused) {
          void win.hide();
        }
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, []);

  // 每次显示时自动聚焦输入框
  useEffect(() => {
    // 小延迟等窗口渲染完毕
    const t = setTimeout(() => inpRef.current?.focus(), 80);
    return () => clearTimeout(t);
  }, []);

  async function send() {
    const t = text.trim();
    if (!t || sending) return;
    setSending(true);
    try {
      const msg: { audio_path: string } = await invoke("generate_tts", { text: t });
      setText("");
      setSent(true);
      // 自动播放语音
      try {
        const url = await getAudioUrl(msg.audio_path);
        const a = new Audio(url);
        void a.play();
      } catch { /* 播放失败不影响发送 */ }
      setTimeout(() => setSent(false), 800);
    } catch (e) {
      window.alert(`发送失败：${e}`);
    } finally {
      setSending(false);
    }
  }

  async function openMain() {
    setMenu(false);
    void await invoke("show_main_window");
  }

  return (
    <div className="flex h-screen flex-col rounded-2xl bg-[var(--paper)] text-[var(--ink-900)] shadow-[0_20px_60px_rgba(26,24,22,0.25)]">
      {/* 发送成功时的遮罩提示 ── 印一枚琥珀签 */}
      {sent && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-[var(--paper)]/95 animate-fade">
          <span className="font-display text-base text-[var(--amber-600)]">✓ 已发送</span>
        </div>
      )}

      <div className="flex items-center gap-1.5 px-3 py-3">
        <input
          ref={inpRef}
          className="flex-1 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm text-[var(--ink-900)] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
          placeholder="输入文字，回车发送…"
          value={text}
          disabled={sending}
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

        {/* 菜单 ── 右侧齿轮点 */}
        <div className="relative">
          <button
            onClick={() => setMenu((v) => !v)}
            className="rounded-lg p-1.5 text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            ⋯
          </button>
          {menu && (
            <>
              <div className="fixed inset-0 z-20" onClick={() => setMenu(false)} />
              <div className="absolute right-0 top-full z-30 mt-1.5 w-40 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1 text-xs text-[var(--ink-700)] shadow-[0_8px_24px_rgba(26,24,22,0.12)] animate-fade overflow-hidden">
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
    </div>
  );
}