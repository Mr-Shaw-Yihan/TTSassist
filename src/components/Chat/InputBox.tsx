// 输入框组件：回车/按钮发送，调 generate_tts
// 大纲 4.2

import { useState, useRef } from "react";

interface Props {
  onSend: (text: string) => Promise<void>;
}

export function InputBox({ onSend }: Props) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

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
    <div className="flex gap-2">
      <input
        ref={inputRef}
        className="flex-1 rounded-lg border border-gray-200 px-3 py-2 text-sm outline-none focus:border-blue-400"
        placeholder="输入要朗读的文字，回车发送..."
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
        className="rounded-lg bg-blue-500 px-4 py-2 text-sm font-medium text-white hover:bg-blue-600 disabled:opacity-50"
        disabled={!text.trim() || sending}
        onClick={send}
      >
        {sending ? "..." : "发送"}
      </button>
    </div>
  );
}