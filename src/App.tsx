// 主界面：消息列表 + 悬浮球音量控件 + 输入框 + 设置
// 大纲 4.1-4.6 端到端打通 MVP。

import { useEffect, useState, useRef, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTauriListen } from "./hooks/useTauriListen";
import { InputBox } from "./components/Chat/InputBox";
import { MessageBubble } from "./components/Chat/MessageBubble";
import { VolumeControl } from "./components/Chat/VolumeSlider";
import { MicToggle } from "./components/Chat/MicToggle";
import { FavoriteList } from "./components/Favorites/FavoriteList";
import { SettingsDrawer } from "./components/Settings/SettingsDrawer";
import { QuickInput } from "./components/QuickInput/QuickInput";
import { useSettingsStore } from "./stores/settingsStore";
import {
  generateTTS,
  listMessages,
  listFavorites,
  getSettings,
  getAudioUrl,
} from "./services/invoke";
import type { Message, Favorite } from "./types";

type Tab = "messages" | "favorites";

function App() {
  // 多窗口路由：检查当前窗口 label
  const win = getCurrentWindow();
  if (win.label === "quick_input") {
    return <QuickInput />;
  }

  const [messages, setMessages] = useState<Message[]>([]);
  const [favorites, setFavorites] = useState<Favorite[]>([]);
  const [tab, setTab] = useState<Tab>("messages");
  const [showDrawer, setShowDrawer] = useState(false);
  const [playingPath, setPlayingPath] = useState<string | null>(null);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // 全局唯一播放元素（互斥播放）
  const playingRef = useRef<HTMLAudioElement | null>(null);
  const playingPathRef = useRef<string | null>(null);

  const reloadFavorites = useCallback(async () => {
    try {
      setFavorites(await listFavorites());
    } catch (e) {
      console.error("加载收藏失败", e);
    }
  }, []);

  // 启动时：加载设置 + 加载消息 + 收藏
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
      await reloadFavorites();
    })();
  }, [setSettings, reloadFavorites]);

  // 皮肤同步到 <html data-theme>，整界面立刻换肤
  useEffect(() => {
    const theme = settings?.theme === "dark" ? "dark" : "light";
    document.documentElement.setAttribute("data-theme", theme);
  }, [settings?.theme]);

  // 首次切到收藏 tab 时加载（列表也兜底）
  useEffect(() => {
    if (tab === "favorites") reloadFavorites();
  }, [tab, reloadFavorites]);

  // 监听 message:changed 事件，重读消息列表（跨窗口同步 + 双保险）
  useTauriListen("message:changed", async () => {
    setMessages(await listMessages());
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
        setPlayingPath(null);
      }
    });
    a.play().catch(() => { /* 用户首次播放可能被忽略 */ });
    playingRef.current = a;
    playingPathRef.current = relPath;
    setPlayingPath(relPath);
  }, [volume, playbackRate]);

  // 监听 favorite:changed，刷新收藏
  useTauriListen("favorite:changed", () => {
    void reloadFavorites();
  }, [reloadFavorites]);

  // 监听 favorite:play（收藏快捷键触发）→ 播扬声器
  useTauriListen<string>("favorite:play", (payload) => {
    void playAudio(payload);
  }, [playAudio]);

  // 监听 settings:changed，重读 settings 到 store（克隆命令在别处改 settings 时同步）
  useTauriListen("settings:changed", async () => {
    try {
      setSettings(await getSettings());
    } catch (e) {
      console.error("重读设置失败", e);
    }
  }, [setSettings]);

  // 兜底：窗口聚焦时同时刷新收藏
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(async ({ payload: focused }) => {
        if (focused) {
          try {
            setMessages(await listMessages());
            await reloadFavorites();
          } catch { /* ignore */ }
        }
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, [reloadFavorites]);

  async function handleSend(text: string) {
    const msg = await generateTTS(text);
    setMessages((prev) => [...prev, msg]);
    // 自动播放延迟 0.4 秒（避免刚生成时卡音）
    setTimeout(() => playAudio(msg.audio_path), 400);
  }

  return (
    <div className="relative flex h-screen flex-col bg-[var(--paper)] text-[var(--ink-900)]">
      {/* 标题栏 ── 品牌字 + 极细底分隔线 */}
      <header className="flex items-center justify-between border-b border-[var(--ink-200)] bg-[var(--paper)] px-4 py-3">
        <div className="flex items-baseline gap-2">
          <span className="font-display text-base text-[var(--ink-900)] tracking-tight">电子声带</span>
          <span className="text-[10px] text-[var(--ink-300)] tracking-[0.3em] uppercase">TTSassist</span>
        </div>
        <div className="flex items-center gap-0.5">
          <MicToggle onOpenSettings={() => setShowDrawer(true)} />
          <VolumeControl />
          <button
            onClick={() => setShowDrawer(true)}
            className="ml-1 rounded-lg p-1.5 text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)] transition-colors"
            title="设置"
          >
            <span className="text-base">⋯</span>
          </button>
        </div>
      </header>

      {needKey && (
        <div className="border-b border-[var(--amber-200)]/60 bg-[var(--amber-200)]/20 px-4 py-2 text-xs text-[var(--amber-600)] animate-fade">
          尚未配置 MiMo API Key，
          <button className="underline underline-offset-2 font-medium" onClick={() => setShowDrawer(true)}>
            点击配置
          </button>
        </div>
      )}

      {/* tab 切换 ── 下划线指示器 */}
      <div className="flex gap-1 border-b border-[var(--ink-200)] bg-[var(--paper)] px-4 text-sm">
        {(["messages", "favorites"] as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={[
              "px-3 py-2.5 -mb-px border-b-2 transition-all duration-200",
              tab === t
                ? "border-[var(--amber-600)] text-[var(--ink-900)] font-medium"
                : "border-transparent text-[var(--ink-300)] hover:text-[var(--ink-700)]",
            ].join(" ")}
          >
            {t === "messages" ? "消息" : "收藏"}
          </button>
        ))}
      </div>

      {/* 消息列表 / 收藏列表 */}
      {tab === "messages" ? (
        <main ref={listRef} className="scrollbar-thin flex-1 space-y-3 overflow-y-auto px-4 py-5">
          {messages.length === 0 ? (
            <div className="mt-16 flex flex-col items-center gap-3 text-[var(--ink-300)] animate-fade">
              <span className="font-display text-3xl text-[var(--ink-200)]">·</span>
              <span className="text-sm">输入要朗读的文字，回车发送</span>
            </div>
          ) : (
            messages.map((m) => (
              <MessageBubble
                key={m.id}
                message={m}
                playingPath={playingPath}
                onDeleted={(id) => setMessages((prev) => prev.filter((x) => x.id !== id))}
                onFavorited={() => { /* 已通过 favorite:changed 事件刷新 */ }}
                onPlay={() => playAudio(m.audio_path)}
              />
            ))
          )}
        </main>
      ) : (
        <main className="flex-1 overflow-hidden bg-[var(--paper)]">
          <FavoriteList
            favorites={favorites}
            playingPath={playingPath}
            onPlay={(p) => playAudio(p)}
            onChanged={reloadFavorites}
          />
        </main>
      )}

      {/* 输入框（仅消息 tab 显示） */}
      {tab === "messages" && (
        <footer className="border-t border-[var(--ink-200)] bg-[var(--paper-card)] p-3">
          <InputBox onSend={handleSend} />
        </footer>
      )}

      {showDrawer && <SettingsDrawer onClose={() => setShowDrawer(false)} />}
    </div>
  );
}

export default App;