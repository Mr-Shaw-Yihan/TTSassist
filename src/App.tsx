// 主界面：消息列表 + 工具栏 + 输入框 + 设置
// 大纲 4.1-4.6 端到端打通 MVP。

import { useEffect, useState, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { InputBox } from "./components/Chat/InputBox";
import { MessageBubble } from "./components/Chat/MessageBubble";
import { Toolbar } from "./components/Chat/VolumeSlider";
import { ApiKeyModal } from "./components/Settings/ApiKeyModal";
import { useSettingsStore } from "./stores/settingsStore";
import {
  generateTTS,
  listMessages,
  getSettings,
  getAudioUrl,
} from "./services/invoke";
import type { Message } from "./types";

function App() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [showApiKey, setShowApiKey] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);

  const playingRef = useRef<HTMLAudioElement | null>(null);

  // 启动时：加载设置 + 加载消息
  useEffect(() => {
    (async () => {
      try {
        const s = await getSettings();
        setSettings(s);
      } catch (e) {
        console.error("加载设置失败", e);
      }
      try {
        const msgs = await listMessages();
        setMessages(msgs);
      } catch (e) {
        console.error("加载消息失败", e);
      }
    })();
  }, [setSettings]);

  // 监听 message:changed 事件，重读消息列表（跨窗口同步 + 双保险）
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await listen("message:changed", async () => {
        const msgs = await listMessages();
        setMessages(msgs);
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, []);

  // 兜底：窗口聚焦时刷新一次
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(async ({ payload: focused }) => {
        if (focused) {
          try {
            setMessages(await listMessages());
          } catch { /* ignore */ }
        }
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, []);

  // 滚动管理：新消息来时滚到底
  const listRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  // 播放音量与播放速度
  const volume = settings?.playback_volume ?? 0.8;
  const playbackRate = settings?.playback_rate ?? 1.0;
  useEffect(() => {
    if (playingRef.current) {
      playingRef.current.volume = volume;
      playingRef.current.playbackRate = playbackRate;
    }
  }, [volume, playbackRate]);

  // 没配 key 头部也提示
  const needKey = !settings?.mimo_api_key;

  // 自动播放某条音频
  async function playAudio(relPath: string) {
    playingRef.current?.pause();
    const url = await getAudioUrl(relPath);
    const a = new Audio(url);
    a.volume = volume;
    a.playbackRate = playbackRate;
    a.play().catch(() => { /* 用户首次播放可能被忽略 */ });
    playingRef.current = a;
  }

  async function handleSend(text: string) {
    const msg = await generateTTS(text);
    // 立即加气泡（不等事件回来），双保险
    setMessages((prev) => [...prev, msg]);
    // 自动播放延迟 0.2 秒，给"已发出"的感知缓冲
    setTimeout(() => playAudio(msg.audio_path), 200);
  }

  return (
    <div className="flex h-screen flex-col bg-gray-50 text-gray-800">
      {/* 标题栏 */}
      <header className="flex items-center justify-between border-b bg-white px-4 py-3">
        <span className="text-sm font-semibold">VoiceAssist</span>
        <button
          onClick={() => setShowSettings((v) => !v)}
          className="rounded px-2 py-1 text-xs text-gray-500 hover:bg-gray-100"
          title="设置"
        >
          ⚙️
        </button>
      </header>

      {needKey && (
        <div className="bg-amber-50 px-4 py-2 text-xs text-amber-700">
          尚未配置 MiMo API Key，
          <button className="underline" onClick={() => setShowApiKey(true)}>
            点击配置
          </button>
        </div>
      )}

      {showSettings && (
        <div className="border-b bg-white px-4 py-2 text-sm">
          <button
            onClick={() => { setShowApiKey(true); }}
            className="text-blue-600 hover:underline"
          >
            设置 MiMo API Key
          </button>
        </div>
      )}

      {/* 消息列表 */}
      <main ref={listRef} className="scrollbar-thin flex-1 space-y-3 overflow-y-auto px-4 py-4">
        {messages.length === 0 ? (
          <div className="mt-8 text-center text-sm text-gray-400">
            输入要朗读的文字，回车发送
          </div>
        ) : (
          messages.map((m) => (
            <MessageBubble
              key={m.id}
              message={m}
              volume={volume}
              playbackRate={playbackRate}
              onDeleted={(id) => setMessages((prev) => prev.filter((x) => x.id !== id))}
              onFavorited={() => { /* P1 阶段收藏夹可见 */ }}
            />
          ))
        )}
      </main>

      {/* 工具栏（音量/播放速度/合成语速） */}
      <div className="border-t bg-white px-4 py-2">
        <Toolbar />
      </div>

      {/* 输入框 */}
      <footer className="border-t bg-white p-3">
        <InputBox onSend={handleSend} />
      </footer>

      {/* API Key Modal */}
      {showApiKey && <ApiKeyModal onClose={() => { setShowApiKey(false); setShowSettings(false); }} />}
    </div>
  );
}

export default App;