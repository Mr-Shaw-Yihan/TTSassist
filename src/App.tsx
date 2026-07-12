import { useState } from "react";

/**
 * 主界面骨架（占位实现）
 * 后续按开发任务清单第一阶段 4.x 实现：消息气泡、TTS 调用、播放、右键菜单、音量调节
 */
function App() {
  const [text, setText] = useState("");

  function send() {
    if (!text.trim()) return;
    // TODO: 调用 invoke("generate_tts", { text }) → 生成消息气泡 + 自动播放
    console.log("send:", text);
    setText("");
  }

  return (
    <div className="flex h-screen flex-col bg-gray-50 text-gray-800">
      {/* 顶部标题栏 */}
      <header className="border-b bg-white px-4 py-3 text-sm font-medium">
        VoiceAssist
      </header>

      {/* 消息列表区 */}
      <main className="scrollbar-thin flex-1 overflow-y-auto px-4 py-3">
        <div className="text-center text-sm text-gray-400">
          消息区域占位（待实现消息气泡）
        </div>
      </main>

      {/* 工具栏（消息框与输入框之间）— 含音量调节 */}
      <div className="flex items-center gap-2 border-t bg-white px-4 py-2">
        <button
          className="rounded px-2 py-1 text-sm text-gray-500 hover:bg-gray-100"
          title="音量调节（待实现）"
        >
          🔊
        </button>
      </div>

      {/* 输入框 */}
      <footer className="border-t bg-white p-3">
        <div className="flex gap-2">
          <input
            className="flex-1 rounded-lg border border-gray-200 px-3 py-2 text-sm outline-none focus:border-blue-400"
            placeholder="输入要朗读的文字，回车发送..."
            value={text}
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
            disabled={!text.trim()}
            onClick={send}
          >
            发送
          </button>
        </div>
      </footer>
    </div>
  );
}

export default App;