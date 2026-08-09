// 主界面：消息列表 + 悬浮球音量控件 + 输入框 + 设置
// 大纲 4.1-4.6 端到端打通 MVP。

import { useEffect, useState, useRef, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTauriListen } from "./hooks/useTauriListen";
import { useVoiceInputHotkey } from "./hooks/useVoiceInputHotkey";
import { InputBox } from "./components/Chat/InputBox";
import { MessageBubble } from "./components/Chat/MessageBubble";
import { VolumeControl } from "./components/Chat/VolumeSlider";
import { MicToggle } from "./components/Chat/MicToggle";
import { FavoriteList } from "./components/Favorites/FavoriteList";
import { SettingsPage } from "./components/Settings/SettingsPage";
import { QuickInput } from "./components/QuickInput/QuickInput";
import { PluginPage } from "./components/Plugins/PluginPage";
import { UpdateDialog } from "./components/Settings/UpdateDialog";
import { useSettingsStore } from "./stores/settingsStore";
import { useUpdateStore, shouldShowUpdateDot } from "./stores/updateStore";
import { usePluginTaskStore } from "./stores/pluginTaskStore";
import {
  generateTTS,
  listMessages,
  listFavorites,
  getSettings,
  getAudioUrl,
  playToMic,
} from "./services/invoke";
import type { Message, Favorite, PluginSetupProgress } from "./types";

type Tab = "messages" | "favorites" | "plugins" | "settings";

/** 消息列表每页条数：首屏只载最近一页，上滑再翻页加载更早的 */
const MESSAGE_PAGE_SIZE = 20;

function App() {
  // 多窗口路由：检查当前窗口 label
  const win = getCurrentWindow();
  if (win.label === "quick_input") {
    return <QuickInput />;
  }

  const [messages, setMessages] = useState<Message[]>([]);
  // 消息分页：是否还有更早的 / 翻页加载中（防重入）
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const loadingOlderRef = useRef(false);
  // 首屏消息是否已加载完成（初始定位滚底的触发器）
  const [initialLoaded, setInitialLoaded] = useState(false);
  // 视口是否贴近底部（新消息自动滚动判据 + 定位按钮显示条件）
  const [atBottom, setAtBottom] = useState(true);
  const [favorites, setFavorites] = useState<Favorite[]>([]);
  const [tab, setTab] = useState<Tab>("messages");
  const [playingPath, setPlayingPath] = useState<string | null>(null);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const patch = useSettingsStore((s) => s.patch);

  // 版本更新状态
  const updateLatest = useUpdateStore((s) => s.latest);
  const dialogDismissed = useUpdateStore((s) => s.dialogDismissed);
  const dismissDialog = useUpdateStore((s) => s.dismissDialog);
  const checkUpdate = useUpdateStore((s) => s.check);
  const updateDot = useUpdateStore(shouldShowUpdateDot);

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

  // 启动时：加载设置 + 加载最近一页消息 + 收藏
  useEffect(() => {
    (async () => {
      try {
        const s = await getSettings();
        setSettings(s);
      } catch (e) {
        console.error("加载设置失败", e);
      }
      try {
        const page = await listMessages(MESSAGE_PAGE_SIZE);
        setMessages(page.messages);
        setHasMoreMessages(page.has_more);
        // 不在此处直接滚底：DOM 尚未提交新消息，滚的是旧高度；
        // 置位后由下方初始定位 effect 在提交后可靠滚底
        setInitialLoaded(true);
      } catch (e) {
        console.error("加载消息失败", e);
      }
      await reloadFavorites();
    })();
  }, [setSettings, reloadFavorites]);

  // 启动时检查一次版本更新（失败静默）
  useEffect(() => {
    void checkUpdate();
  }, [checkUpdate]);

  // 皮肤同步到 <html data-theme>，整界面立刻换肤
  useEffect(() => {
    const theme = settings?.theme === "dark" ? "dark" : "light";
    document.documentElement.setAttribute("data-theme", theme);
  }, [settings?.theme]);

  // 首次切到收藏 tab 时加载（列表也兜底）
  useEffect(() => {
    if (tab === "favorites") reloadFavorites();
  }, [tab, reloadFavorites]);

  // 监听 message:changed 事件（跨窗口同步：浮窗也会生成消息）：
  // 重取最新一页并与已加载的更早消息合并（去重），不打断用户向上翻阅
  useTauriListen("message:changed", async () => {
    await reloadLatestMessages();
  }, []);

  /** 重取最新一页并合并：保留已加载且不在新窗口里的更早消息 */
  const reloadLatestMessages = useCallback(async () => {
    try {
      const page = await listMessages(MESSAGE_PAGE_SIZE);
      setHasMoreMessages(page.has_more);
      setMessages((prev) => {
        const inWindow = new Set(page.messages.map((m) => m.id));
        const older = prev.filter((m) => !inWindow.has(m.id));
        return [...older, ...page.messages];
      });
    } catch { /* 刷新失败静默，下次事件再试 */ }
  }, []);

  /** 上滑翻页：加载更早一页并 prepend，保持视口位置不跳 */
  const loadOlderMessages = useCallback(async (firstId?: string) => {
    if (loadingOlderRef.current || !firstId) return;
    const el = listRef.current;
    if (!el) return;
    loadingOlderRef.current = true;
    const distFromBottom = el.scrollHeight - el.scrollTop;
    try {
      const page = await listMessages(MESSAGE_PAGE_SIZE, firstId);
      setHasMoreMessages(page.has_more);
      if (page.messages.length > 0) {
        setMessages((prev) => [...page.messages, ...prev]);
        requestAnimationFrame(() => {
          const node = listRef.current;
          if (node) node.scrollTop = node.scrollHeight - distFromBottom;
        });
      }
    } catch (e) {
      console.error("加载更早消息失败", e);
    } finally {
      loadingOlderRef.current = false;
    }
  }, []);

  /** 消息列表滚动：接近顶部时翻上一页；记录是否在底部（新消息自动滚动/定位按钮判据） */
  function onMessagesScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    setAtBottom(el.scrollHeight - el.scrollTop - el.clientHeight < 60);
    if (el.scrollTop < 80 && hasMoreMessages && !loadingOlderRef.current) {
      void loadOlderMessages(messages[0]?.id);
    }
  }

  // 初始定位：首屏消息提交到 DOM 后滚到底（每次启动默认看最新），
  // 150ms 后再补一次（音频/字体等内容异步撑高也能到底）
  useEffect(() => {
    if (!initialLoaded) return;
    const run = () => {
      const el = listRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    };
    run();
    const t = window.setTimeout(run, 150);
    return () => window.clearTimeout(t);
  }, [initialLoaded]);

  // 滚动管理：滚到底工具（定位按钮/新消息用；翻页加载保持视口不用它）。
  // 双保险：rAF 等一次布局，50ms 后再补一次（内容异步撑高时也能到底）
  const listRef = useRef<HTMLDivElement | null>(null);
  const scrollToBottom = useCallback(() => {
    const run = () => {
      const el = listRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    };
    requestAnimationFrame(run);
    setTimeout(run, 50);
  }, []);

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

  // 手动播放（收藏/消息重播）：扬声器 + 若全局开关开启则同时发虚拟麦克风。
  // 与快捷键路径对齐（快捷键在后端 hotkey 回调里发麦克风）；
  // 生成后的自动播放不走这里（generate_tts 后端已发过，避免双份）。
  const micEnabled = settings?.mic_send_enabled ?? false;
  const micDevice = settings?.mic_output_device ?? "";
  const micVolume = settings?.mic_playback_volume ?? 1.0;
  const playAudioWithMic = useCallback(async (relPath: string) => {
    await playAudio(relPath);
    if (micEnabled && micDevice.trim()) {
      try {
        await playToMic(relPath, micDevice, micVolume);
      } catch (e) {
        console.error("发送到虚拟麦克风失败", e);
      }
    }
  }, [playAudio, micEnabled, micDevice, micVolume]);

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

  // 语音输入全局快捷键会话（按住说话）：录音状态展示在 InputBox
  useVoiceInputHotkey();

  // 插件安装进度 → 全局任务 store（面板只订阅 store，启动在用户动作处）
  const applyTaskProgress = usePluginTaskStore((s) => s.applyProgress);
  useTauriListen<PluginSetupProgress>(
    "plugin-setup-progress",
    (p) => applyTaskProgress(p),
    [applyTaskProgress],
  );

  // 兜底：窗口聚焦时刷新收藏 + 静默同步最新一页消息
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    (async () => {
      const u = await win.onFocusChanged(async ({ payload: focused }) => {
        if (focused) {
          try {
            await reloadLatestMessages();
            await reloadFavorites();
          } catch { /* ignore */ }
        }
      });
      unlisten = u;
    })();
    return () => { unlisten?.(); };
  }, [reloadLatestMessages, reloadFavorites]);

  async function handleSend(text: string) {
    const msg = await generateTTS(text);
    setMessages((prev) => [...prev, msg]);
    // 新消息滚到底（用户正在向上翻阅时不打断，可用定位按钮回底部）
    if (atBottom) scrollToBottom();
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
          <MicToggle onOpenSettings={() => setTab("settings")} />
          <VolumeControl />
        </div>
      </header>

      {needKey && (
        <div className="border-b border-[var(--amber-200)]/60 bg-[var(--amber-200)]/20 px-4 py-2 text-xs text-[var(--amber-600)] animate-fade">
          尚未配置 MiMo API Key，
          <button className="underline underline-offset-2 font-medium" onClick={() => setTab("settings")}>
            点击配置
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        {/* 左侧边栏（永久）：消息 / 收藏 / 插件 / 设置 */}
        <nav className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-[var(--ink-200)] bg-[var(--paper)] py-3">
          <SideButton icon="💬" label="消息" active={tab === "messages"} onClick={() => setTab("messages")} />
          <SideButton icon={<StarIcon />} label="收藏" active={tab === "favorites"} onClick={() => setTab("favorites")} />
          <SideButton icon="⧉" label="插件" active={tab === "plugins"} onClick={() => setTab("plugins")} />
          <div className="flex-1" />
          <SideButton icon="⋯" label="设置" active={tab === "settings"} dot={updateDot} onClick={() => setTab("settings")} />
        </nav>

        {/* 右侧内容区（随侧边栏切换） */}
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {/* 消息列表（首屏仅最近一页，上滑翻页加载更早） */}
          {tab === "messages" && (
            <div className="relative flex min-h-0 flex-1 flex-col">
              <main
                ref={listRef}
                onScroll={onMessagesScroll}
                className="scrollbar-thin flex-1 space-y-3 overflow-y-auto px-4 py-5"
              >
              {hasMoreMessages && (
                <div className="py-1 text-center text-[10px] text-[var(--ink-300)]">
                  {loadingOlderRef.current ? "正在加载更早的消息…" : "向上滑动查看更早的消息"}
                </div>
              )}
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
                    onPlay={() => playAudioWithMic(m.audio_path)}
                  />
                ))
              )}
              </main>

              {/* 定位到底部（向上翻阅后一键回到最新消息） */}
              {messages.length > 0 && !atBottom && (
                <button
                  onClick={scrollToBottom}
                  title="定位到最新消息"
                  className="absolute bottom-4 left-4 z-10 flex items-center gap-1 rounded-full border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-1.5 text-[11px] text-[var(--ink-500)] shadow-[0_2px_8px_rgba(26,24,22,0.08)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] animate-fade"
                >
                  <span aria-hidden>↓</span> 最新
                </button>
              )}
            </div>
          )}

          {/* 收藏列表 */}
          {tab === "favorites" && (
            <main className="min-h-0 flex-1 overflow-hidden bg-[var(--paper)]">
              <FavoriteList
                favorites={favorites}
                playingPath={playingPath}
                onPlay={(p) => playAudioWithMic(p)}
                onChanged={reloadFavorites}
              />
            </main>
          )}

          {/* 插件管理 */}
          {tab === "plugins" && (
            <main className="min-h-0 flex-1 overflow-hidden bg-[var(--paper)]">
              <PluginPage />
            </main>
          )}

          {/* 设置 */}
          {tab === "settings" && (
            <main className="min-h-0 flex-1 overflow-hidden bg-[var(--paper)]">
              <SettingsPage />
            </main>
          )}

          {/* 输入框（仅消息视图显示） */}
          {tab === "messages" && (
            <footer className="border-t border-[var(--ink-200)] bg-[var(--paper-card)] p-3">
              <InputBox onSend={handleSend} />
            </footer>
          )}
        </div>
      </div>

      {/* 版本更新弹窗（有新版本且未忽略该版本时，每次启动提示一次） */}
      {updateLatest &&
        !dialogDismissed &&
        updateLatest.version !== (settings?.update_ignored_version ?? "") && (
          <UpdateDialog
            info={updateLatest}
            onLater={dismissDialog}
            onIgnore={async () => {
              try {
                await patch("update_ignored_version", updateLatest.version);
              } catch {
                /* 忽略失败不影响关闭 */
              }
              dismissDialog();
            }}
          />
        )}
    </div>
  );
}

/** 侧边栏按钮：图标 + 小字标签，可选红点 */
function SideButton({
  icon,
  label,
  active,
  dot,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  dot?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={[
        "relative flex h-11 w-11 flex-col items-center justify-center gap-0.5 rounded-xl transition-colors",
        active
          ? "bg-[var(--amber-200)]/40 text-[var(--amber-600)]"
          : "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
      ].join(" ")}
    >
      <span className="flex h-4 w-4 items-center justify-center text-base leading-none">{icon}</span>
      <span className="text-[9px] leading-none tracking-wide">{label}</span>
      {dot && (
        <span className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-[var(--seal)]" />
      )}
    </button>
  );
}

/** 简约星星图标（收藏用） */
function StarIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  );
}

export default App;