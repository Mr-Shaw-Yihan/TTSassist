// 主界面：消息列表 + 悬浮球音量控件 + 输入框 + 设置
// 大纲 4.1-4.6 端到端打通 MVP。

import { useEffect, useState, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { InputBox } from "./components/Chat/InputBox";
import { MessageBubble } from "./components/Chat/MessageBubble";
import { FloatingBall } from "./components/Chat/VolumeSlider";
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
  const [playingId, setPlayingId] = useState<string | null>(null);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // 全局唯一播放元素（互斥播放）
  const playingRef = useRef<HTMLAudioElement | null>(null);
  // 记录当前正在播放的 audio_path，用于 ended 回调判断是否还是这条
  const playingPathRef = useRef<string | null>(null);

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

  // 互斥播放：停旧 → 设新 → 播放
  const playAudio = useCallback(async (relPath: string) => {
    playingRef.current?.pause();
    const url = await getAudioUrl(relPath);
    const a = new Audio(url);
    a.volume = volume;
    a.playbackRate = playbackRate;
    // 仅当此条仍是当前在播时才清高亮（切到其它音频时它被 pause，不会触发 ended）
    a.addEventListener("ended", () => {
      if (playingPathRef.current === relPath) {
        playingPathRef.current = null;
        setPlayingId(null);
      }
    });
    a.play().catch(() => { /* 用户首次播放可能被忽略 */ });
    playingRef.current = a;
    playingPathRef.current = relPath;
  }, [volume, playbackRate]);

  // MessageBubble 传的 onPlay 需要带消息 id 维度更新正在播放态
  // 因为 audioPath 不等于 id（同音频可能被收藏引用），改为接收 audioPath + 当前气泡 id
  function onBubblePlay(msg: Message) {
    setPlayingId(msg.id);
    playAudio(msg.audio_path);
  }

  async function handleSend(text: string) {
    const msg = await generateTTS(text);
    setMessages((prev) => [...prev, msg]);
    // 自动播放延迟 0.4 秒（避免刚生成时卡音）
    setTimeout(() => onBubblePlay(msg), 400);
  }

  return (
    <div className="relative flex h-screen flex-col bg-gray-50 text-gray-800">
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
              playingId={playingId}
              onDeleted={(id) => setMessages((prev) => prev.filter((x) => x.id !== id))}
              onFavorited={() => { /* P1 阶段收藏夹可见 */ }}
              onPlay={() => onBubblePlay(m)}
            />
          ))
        )}
      </main>

      {/* 输入框 */}
      <footer className="border-t bg-white p-3">
        <InputBox onSend={handleSend} />
      </footer>

      {/* 悬浮球音量控件（绝对定位，浮在界面之上） */}
      <FloatingBall />

      {/* API Key Modal */}
      {showApiKey && <ApiKeyModal onClose={() => { setShowApiKey(false); setShowSettings(false); }} />}
    </div>
  );
}

export default App;