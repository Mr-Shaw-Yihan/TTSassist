// 主界面：消息列表 + 悬浮球音量控件 + 输入框 + 设置
// 大纲 4.1-4.6 端到端打通 MVP。

import { useEffect, useState, useRef, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { useTauriListen } from "./hooks/useTauriListen";
import { useVoiceInputHotkey } from "./hooks/useVoiceInputHotkey";
import { InputBox } from "./components/Chat/InputBox";
import { MessageBubble } from "./components/Chat/MessageBubble";
import { VolumeControl } from "./components/Chat/VolumeSlider";
import { MicToggle } from "./components/Chat/MicToggle";
import { MicIcon } from "./components/icons/MicIcon";
import { TexDefs, TexIcon } from "./components/icons/TexIcon";
import { FavoriteList } from "./components/Favorites/FavoriteList";
import { SettingsPage } from "./components/Settings/SettingsPage";
import { QuickInput } from "./components/QuickInput/QuickInput";
import { FloatingBall } from "./components/FloatingBall/FloatingBall";
import { BallLogo } from "./components/FloatingBall/BallLogo";
import { PluginPage } from "./components/Plugins/PluginPage";
import { UpdateDialog } from "./components/Settings/UpdateDialog";
import { useSettingsStore } from "./stores/settingsStore";
import { useUpdateStore, shouldShowUpdateDot } from "./stores/updateStore";
import { usePluginTaskStore } from "./stores/pluginTaskStore";
import { playMicOnChime, playMicOffChime } from "./utils/chime";
import {
  generateTTS,
  listMessages,
  listFavorites,
  getSettings,
  getAudioUrl,
  playToMic,
  stopMic,
  listPlugins,
  promptEngineWarmup,
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
  if (win.label === "floating_ball") {
    return <FloatingBall />;
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
  // 「其他」弹层（收纳麦克风开关、音量与播放速度）
  const [moreOpen, setMoreOpen] = useState(false);
  const moreRef = useRef<HTMLDivElement | null>(null);
  const [playingPath, setPlayingPath] = useState<string | null>(null);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const patch = useSettingsStore((s) => s.patch);
  // 麦克风发送状态指示（标题栏图标）：开关开启且已配置设备 = 生效中（绿色），与 MicToggle 口径一致
  const micSendOn =
    (settings?.mic_send_enabled ?? false) &&
    !!(settings?.mic_output_device && settings.mic_output_device.trim());

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

  // 启动时：当前引擎若为已就绪的本地引擎，询问是否后台预热
  //（避免第一次对话时现场加载模型久等；通用机制，适用所有 category=local 引擎）
  const warmupAskedRef = useRef(false);
  useEffect(() => {
    if (!settings || warmupAskedRef.current) return;
    const engineId = settings.tts_engine ?? "mimo";
    void (async () => {
      try {
        const plugins = await listPlugins();
        const p = plugins.find((x) => x.id === engineId);
        if (p?.category === "local" && p.setup_status?.ready) {
          const voiceId = settings.plugin_voices?.[p.id] ?? p.voices[0]?.id ?? "";
          if (!voiceId) return;
          warmupAskedRef.current = true; // 无论用户是否确认，本次启动只问一次
          // 稍延再弹，避开首屏渲染与更新弹窗的峰值
          setTimeout(() => void promptEngineWarmup(p.name, p.id, voiceId), 800);
        }
      } catch {
        /* 插件列表拉取失败静默，不影响主流程 */
      }
    })();
  }, [settings]);

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

  /** 消息列表滚动：接近顶部时翻上一页；记录是否在底部（新消息自动滚动/定位按钮判据）；
   *  持续记录滚动位置，供切 tab 回来后恢复（条件渲染重挂载会丢 scrollTop） */
  function onMessagesScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    setAtBottom(near);
    savedScrollRef.current = el.scrollTop;
    savedAtBottomRef.current = near;
    if (el.scrollTop < 80 && hasMoreMessages && !loadingOlderRef.current) {
      void loadOlderMessages(messages[0]?.id);
    }
  }

  // 切回消息 tab 时恢复位置：列表 DOM 重建后 scrollTop 归零，
  // 按离开前连续记录的位置恢复；离开前在底部则恢复为贴底（期间来了新消息也能看到最新）
  const savedScrollRef = useRef<number | null>(null);
  const savedAtBottomRef = useRef(false);
  useEffect(() => {
    if (tab !== "messages" || !initialLoaded) return;
    const saved = savedScrollRef.current;
    if (saved === null) return;
    const run = () => {
      const el = listRef.current;
      if (!el) return;
      el.scrollTop = savedAtBottomRef.current ? el.scrollHeight : saved;
    };
    run();
    requestAnimationFrame(run);
  }, [tab, initialLoaded]);

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

  // 点弹层外收起「其他」面板
  useEffect(() => {
    if (!moreOpen) return;
    function onClick(e: MouseEvent) {
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) {
        setMoreOpen(false);
      }
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [moreOpen]);

  // 播放音量与播放速度
  const volume = settings?.playback_volume ?? 0.8;
  const playbackRate = settings?.playback_rate ?? 1.0;
  useEffect(() => {
    if (playingRef.current) {
      playingRef.current.volume = volume;
      playingRef.current.playbackRate = playbackRate;
    }
  }, [volume, playbackRate]);

  // 停止当前播放（扬声器 + 虚拟麦克风）：手动再点停止与后端 playback:stop 事件共用。
  // playback:stop 是通用播放控制（宿主能力桥 stop_playback 发出，非遥控特供）。
  const stopPlayback = useCallback(() => {
    const cur = playingRef.current;
    if (cur && !cur.paused) {
      cur.pause();
    }
    playingRef.current = null;
    playingPathRef.current = null;
    setPlayingPath(null);
    // 虚拟麦克风侧可能正在播同一条，同步停止（未在播时无副作用）
    stopMic().catch(() => {});
    void emit("va:play:stop").catch(() => {});
    void emit("playback:stopped").catch(() => {});
  }, []);

  // 互斥播放：停旧 → 设新 → 播放；再次点击正在播的同一条 → 停止（扬声器 + 虚拟麦克风）。
  // 返回 true=开始播放 / false=停止，供 playAudioWithMic 判断是否还要发麦克风。
  // 播放开始/结束同时发通用 playback:started/stopped 事件（后端聚合为播放态，
  // 供能力桥订阅方（如手机遥控）感知，与 playback:play-last 同族命名）。
  const playAudio = useCallback(async (relPath: string): Promise<boolean> => {
    const cur = playingRef.current;
    if (playingPathRef.current === relPath && cur && !cur.paused) {
      stopPlayback();
      return false;
    }
    const wasPlaying = !!cur && !cur.paused;
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
      void emit("va:play:stop").catch(() => {});
      void emit("playback:stopped").catch(() => {});
    });
    a.play().catch(() => { /* 用户首次播放可能被忽略 */ });
    if (wasPlaying) {
      void emit("va:play:stop").catch(() => {});
      void emit("playback:stopped").catch(() => {});
    }
    void emit("va:play:start").catch(() => {});
    void emit("playback:started", relPath).catch(() => {});
    playingRef.current = a;
    playingPathRef.current = relPath;
    setPlayingPath(relPath);
    return true;
  }, [volume, playbackRate, stopPlayback]);

  // 手动播放（收藏/消息重播）：扬声器 + 若全局开关开启则同时发虚拟麦克风。
  // 与快捷键路径对齐（快捷键在后端 hotkey 回调里发麦克风）；
  // 生成后的自动播放不走这里（generate_tts 后端已发过，避免双份）。
  const micEnabled = settings?.mic_send_enabled ?? false;
  const micDevice = settings?.mic_output_device ?? "";
  const micVolume = settings?.mic_playback_volume ?? 1.0;

  // 麦克风发送状态 → 悬浮球角色徽标事件（va:mic:on/off，右上角绿色小球）
  const micOn = micEnabled && !!micDevice.trim();
  useEffect(() => {
    void emit(micOn ? "va:mic:on" : "va:mic:off").catch(() => {});
  }, [micOn]);
  const playAudioWithMic = useCallback(async (relPath: string) => {
    const started = await playAudio(relPath);
    if (!started) return; // 再次点击停止：不再发麦克风
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

  // 「播放最近一条消息」全局快捷键：用 ref 拿最新消息（避免闭包捕获旧值），
  // 走 playAudioWithMic（扬声器 + 开关开启时发虚拟麦克风，与手动重播一致）
  const messagesRef = useRef<Message[]>([]);
  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);
  useTauriListen("playback:play-last", () => {
    const last = messagesRef.current[messagesRef.current.length - 1];
    if (last) void playAudioWithMic(last.audio_path);
  }, [playAudioWithMic]);

  // 通用播放控制（宿主能力桥发出，供桥接插件等外部触发方使用）：
  // - playback:play{path}：播指定音频（扬声器；麦克风由后端已发，避免双份）
  // - playback:stop：停止当前播放（扬声器 + 虚拟麦克风）
  useTauriListen<string>("playback:play", (payload) => {
    if (payload) void playAudio(payload);
  }, [playAudio]);
  useTauriListen("playback:stop", () => {
    stopPlayback();
  }, [stopPlayback]);

  // 监听 settings:changed，重读 settings 到 store（克隆命令在别处改 settings 时同步）
  useTauriListen("settings:changed", async () => {
    try {
      setSettings(await getSettings());
    } catch (e) {
      console.error("重读设置失败", e);
    }
  }, [setSettings]);

  // 「发送到麦克风」开关音效：开启上行音 / 关闭下行音。
  // 三条切换路径（主窗按钮 / 浮窗按钮 / 全局快捷键）最终都落在这个 store 值上，
  // 此处集中监听，避免多个 MicToggle 实例重复播音；首次加载不播。
  const micSendRef = useRef<boolean | null>(null);
  useEffect(() => {
    const cur = settings?.mic_send_enabled ?? false;
    const prev = micSendRef.current;
    micSendRef.current = cur;
    if (prev === null || prev === cur) return;
    if (cur) playMicOnChime();
    else playMicOffChime();
  }, [settings?.mic_send_enabled]);

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
      {/* 纹理图标/按钮的全局定义（金磨砂 + 紫星点 pattern 与符号库） */}
      <TexDefs />
      {/* 自定义标题栏（系统标题栏已关）：品牌字 + 窗口控制，品牌区可拖拽移动窗口 */}
      <header
        data-tauri-drag-region
        className="flex h-11 shrink-0 items-center justify-between border-b border-[var(--ink-200)] bg-[var(--paper)] pl-4"
      >
        <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center gap-2">
          {/* 悬浮球 logo（休眠态占位）：点击放出/收回悬浮球 */}
          <BallLogo />
          <span className="font-display text-sm text-[var(--ink-900)] tracking-tight">电子声带</span>
          <span className="text-[9px] text-[var(--ink-300)] tracking-[0.3em] uppercase">TTSassist</span>
          {/* 麦克风发送状态指示：开启发绿，未开启为灰色描边 */}
          <span
            title={micSendOn ? "发送到麦克风：已开启" : "发送到麦克风：未开启"}
            className="self-center"
          >
            <MicIcon
              size={13}
              filled={micSendOn}
              className={["transition-colors", micSendOn ? "text-emerald-600" : "text-[var(--ink-300)]"].join(" ")}
            />
          </span>
        </div>
        {/* 窗口控制：最小化 / 最大化 / 关闭 */}
        <div className="flex h-full shrink-0 items-stretch">
          <button
            onClick={() => win.minimize()}
            title="最小化"
            aria-label="最小化"
            className="flex w-11 items-center justify-center text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
              <path d="M1 5h8" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            </svg>
          </button>
          <button
            onClick={() => win.toggleMaximize()}
            title="最大化/还原"
            aria-label="最大化或还原"
            className="flex w-11 items-center justify-center text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden>
              <rect x="1" y="1" width="8" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
            </svg>
          </button>
          <button
            onClick={() => win.close()}
            title="关闭"
            aria-label="关闭"
            className="flex w-11 items-center justify-center text-[var(--ink-300)] transition-colors hover:bg-[var(--seal)] hover:text-white"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
              <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            </svg>
          </button>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        {/* 左侧边栏（永久）：消息 / 收藏 / 插件 / 设置（恒为按钮组最后一个），「其他」置底 */}
        <nav className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-[var(--ink-200)] bg-[var(--paper)] py-3">
          <SideButton icon={<TexIcon name="msg" size={16} />} label="消息" active={tab === "messages"} onClick={() => setTab("messages")} />
          <SideButton icon={<TexIcon name="star" size={16} />} label="收藏" active={tab === "favorites"} onClick={() => setTab("favorites")} />
          <SideButton icon={<TexIcon name="grid" size={16} />} label="插件" active={tab === "plugins"} onClick={() => setTab("plugins")} />
          <SideButton icon={<TexIcon name="gear" size={16} />} label="设置" active={tab === "settings"} dot={updateDot} onClick={() => setTab("settings")} />
          <div className="flex-1" />

          {/* 「其他」：收纳麦克风开关、音量与播放速度 */}
          <div ref={moreRef} className="relative flex justify-center">
            <SideButton
              icon={<TexIcon name="dots" size={16} />}
              label="其他"
              active={moreOpen}
              onClick={() => setMoreOpen((v) => !v)}
            />
            {moreOpen && (
              <div className="absolute bottom-0 left-full z-50 ml-2 flex w-48 flex-col gap-2.5 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-3 shadow-[0_8px_24px_rgba(26,24,22,0.12)] animate-fade">
                <MicToggle
                  variant="row"
                  onOpenSettings={() => {
                    setMoreOpen(false);
                    setTab("settings");
                  }}
                />
                <VolumeControl inline />
              </div>
            )}
          </div>
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
                  aria-label="定位到最新消息"
                  className="absolute bottom-4 left-4 z-10 flex h-9 w-9 items-center justify-center rounded-full border border-[var(--ink-200)] bg-[var(--paper-card)] text-[var(--ink-500)] shadow-[0_2px_8px_rgba(26,24,22,0.08)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] animate-fade"
                >
                  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
                    <path
                      d="M8 2v9m0 0 3.5-3.5M8 11 4.5 7.5M3 13.5h10"
                      stroke="currentColor"
                      strokeWidth="1.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
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

/** 侧边栏按钮：纹理图标 + 小字标签，可选红点。
 *  选中态铺皮肤质感（浅色哑金磨砂 / 深色紫晶星点），图标变为实心断线形态 */
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
          ? "btn-tex border"
          : "text-[var(--ink-300)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]",
      ].join(" ")}
    >
      <span className="flex h-4 w-4 items-center justify-center leading-none">{icon}</span>
      <span className="text-[9px] leading-none tracking-wide">{label}</span>
      {dot && (
        <span className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-[var(--seal)]" />
      )}
    </button>
  );
}

export default App;