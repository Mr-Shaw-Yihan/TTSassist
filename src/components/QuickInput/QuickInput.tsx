// 快捷输入浮窗：极简输入框，发送后"已发送"提示，点击外部自动隐藏。
// 大纲 4.7 + 10.x

import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
      await invoke("generate_tts", { text: t });
      setText("");
      setSent(true);
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
    <div className="flex h-screen flex-col bg-gray-900/95 text-white">
      {/* 发送成功时的遮罩提示 */}
      {sent && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-gray-900/80">
          <span className="text-lg font-medium text-green-400">✅ 已发送</span>
        </div>
      )}

      <div className="flex items-center gap-1 px-2 py-1.5">
        <input
          ref={inpRef}
          className="flex-1 rounded-md border border-white/20 bg-white/10 px-3 py-2 text-sm text-white placeholder-white/40 outline-none focus:border-blue-400"
          placeholder="输入文字，回车发送..."
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
          className="rounded-md bg-blue-500 px-3 py-2 text-sm font-medium hover:bg-blue-600 disabled:opacity-50"
        >
          {sending ? "…" : "发送"}
        </button>

        {/* 齿轮菜单 */}
        <div className="relative">
          <button
            onClick={() => setMenu((v) => !v)}
            className="rounded p-1.5 text-white/60 hover:bg-white/10"
          >
            ⚙️
          </button>
          {menu && (
            <>
              <div className="fixed inset-0 z-20" onClick={() => setMenu(false)} />
              <div className="absolute right-0 top-full z-30 mt-1 w-40 rounded-lg border border-white/10 bg-gray-800 py-1 text-xs shadow-xl">
                <button
                  onClick={openMain}
                  className="block w-full px-3 py-2 text-left hover:bg-white/10"
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