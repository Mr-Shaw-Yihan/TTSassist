// 字幕浮窗（subtitle_window）：透明 / 置顶 / 免焦点 / 鼠标穿透的纯展示叠加层。
// 自身监听 subtitle:state（开始/停止）与 subtitle:preview（预览）事件完成显隐与定位，
// 不依赖主窗「字幕」页是否挂载 —— 热键或命令触发后，浮窗自己出现/收起。
// 高对比配色（白字 + 描边 + 黑底）固定，不随皮肤变化，保证游戏画面上始终可读。

import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow, primaryMonitor, PhysicalPosition } from "@tauri-apps/api/window";
import { useTauriListen } from "../../hooks/useTauriListen";
import { getSettings, subtitleStatus } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import type { SubtitleLine } from "../../types";

/**
 * 把一段（可能很长的）转写文本拆成适合浮窗显示的短条：
 * 优先按标点断句（。！？，、；：…），无标点或仍超长则按字数硬切；
 * 相邻短句会尽量合并到 ≤ maxChars，避免碎片化。maxChars 按中文字数计。
 */
function splitSubtitleText(text: string, maxChars = 22): string[] {
  const clean = text.replace(/\s+/g, " ").trim();
  if (!clean) return [];
  // 每个 token = 一段正文 + 可选尾随标点（标点跟随前文，不单独成条）
  const tokens = clean.match(/[^，。、；：！？,.;:!?\n]+[，。、；：！？,.;:!?\n]?/g) || [clean];
  const out: string[] = [];
  let buf = "";
  const flush = () => {
    const t = buf.trim();
    if (t) out.push(t);
    buf = "";
  };
  for (let tk of tokens) {
    tk = tk.trim();
    if (!tk) continue;
    if (buf && buf.length + tk.length > maxChars) flush();
    while (tk.length > maxChars) {
      out.push(tk.slice(0, maxChars));
      tk = tk.slice(maxChars);
    }
    buf += tk;
  }
  flush();
  return out;
}

export function SubtitleWindow() {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);

  const [lines, setLines] = useState<SubtitleLine[]>([]);
  const [running, setRunning] = useState(false);
  const [paused, setPaused] = useState(false);
  // 心跳时钟：驱动自动淡出与过期清理（500ms 一跳，足够顺滑且开销极低）
  const [now, setNow] = useState(() => Date.now());

  const maxLines = settings?.subtitle_max_lines ?? 3;
  const fontSize = settings?.subtitle_font_size ?? 18;
  const opacity = settings?.subtitle_opacity ?? 0.6;
  const position = settings?.subtitle_position ?? "bottom";
  const fadeSeconds = settings?.subtitle_fade_seconds ?? 15;

  // 事件回调里读最新 position / running（避免闭包捕获旧值）
  const positionRef = useRef(position);
  positionRef.current = position;
  const runningRef = useRef(running);
  runningRef.current = running;

  /** 定位到屏幕底部 / 顶部居中（物理像素，避开任务栏用 workArea）并显示；整窗鼠标穿透 */
  const positionAndShow = useCallback(async () => {
    const w = getCurrentWindow();
    const mon = await primaryMonitor().catch(() => null);
    const size = await w.innerSize().catch(() => null);
    if (mon && size) {
      const wa = mon.workArea;
      const scale = mon.scaleFactor || 1;
      const margin = Math.round(24 * scale);
      const x = wa.position.x + Math.round((wa.size.width - size.width) / 2);
      const y =
        positionRef.current === "top"
          ? wa.position.y + margin
          : wa.position.y + wa.size.height - size.height - margin;
      await w.setPosition(new PhysicalPosition(x, y)).catch(() => {});
    }
    // 纯叠加层：始终鼠标穿透，绝不挡游戏 / 其他程序点击
    await w.setIgnoreCursorEvents(true).catch(() => {});
    await w.show().catch(() => {});
  }, []);

  // 挂载：文档背景透明（否则 body 的 --paper 会把透明窗填成一整块奶白）+ 穿透 + 若已运行则显示
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    const root = document.getElementById("root");
    html.style.background = "transparent";
    body.style.background = "transparent";
    if (root) root.style.background = "transparent";
    void getCurrentWindow().setIgnoreCursorEvents(true).catch(() => {});
    getSettings().then(setSettings).catch(() => {});
    subtitleStatus()
      .then((s) => {
        setRunning(s.running);
        setPaused(s.paused);
        if (s.running) void positionAndShow();
      })
      .catch(() => {});
  }, [setSettings, positionAndShow]);

  // 置顶随设置即时生效
  useEffect(() => {
    void getCurrentWindow()
      .setAlwaysOnTop(settings?.subtitle_always_on_top ?? true)
      .catch(() => {});
  }, [settings?.subtitle_always_on_top]);

  // 心跳
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(id);
  }, []);

  // 新字幕：把长文本拆成短条后逐条上屏（旧在前、新在后 → 底部为最新），同屏截断到 maxLines
  useTauriListen<SubtitleLine>(
    "asr:subtitle",
    (l) => {
      const clauses = splitSubtitleText(l.text);
      if (clauses.length === 0) return;
      setLines((prev) => {
        const add: SubtitleLine[] = clauses.map((t, i) => ({ text: t, ts: l.ts + i }));
        const next = [...prev, ...add];
        return next.length > maxLines ? next.slice(next.length - maxLines) : next;
      });
    },
    [maxLines],
  );

  // 会话启停：运行→清空并显示浮窗；停止→隐藏浮窗
  useTauriListen<{ running: boolean; paused: boolean }>(
    "subtitle:state",
    (p) => {
      setRunning(p.running);
      setPaused(p.paused);
      if (p.running) {
        setLines([]);
        void positionAndShow();
      } else {
        void getCurrentWindow().hide().catch(() => {});
      }
    },
    [positionAndShow],
  );
  // 暂停态切换
  useTauriListen<{ paused: boolean }>(
    "subtitle:paused",
    (p) => setPaused(p.paused),
    [],
  );

  // 预览：管理页点「预览浮窗」→ 显示并放一条示例；「隐藏浮窗」→ 收起
  useTauriListen(
    "subtitle:preview",
    () => {
      void positionAndShow();
      setLines([{ text: "这是字幕浮窗的显示效果预览", ts: Date.now() }]);
    },
    [positionAndShow],
  );
  useTauriListen("subtitle:preview-hide", () => {
    void getCurrentWindow().hide().catch(() => {});
  }, []);

  // 设置变化（主窗改的）：重读换肤/换尺寸；运行中则按新位置重定位
  useTauriListen(
    "settings:changed",
    () => {
      getSettings()
        .then((s) => {
          setSettings(s);
          if (runningRef.current) void positionAndShow();
        })
        .catch(() => {});
    },
    [setSettings, positionAndShow],
  );

  // 过期清理：淡出秒数 > 0 时，超过窗口的旧字幕移出
  useEffect(() => {
    if (fadeSeconds <= 0) return;
    const cutoff = fadeSeconds * 1000;
    setLines((prev) =>
      prev.some((l) => now - l.ts > cutoff)
        ? prev.filter((l) => now - l.ts <= cutoff)
        : prev,
    );
  }, [now, fadeSeconds]);

  const showIdleChip = running && !paused && lines.length === 0;
  const showPausedChip = running && paused;
  const bottom = position !== "top";

  return (
    <div
      className={[
        "pointer-events-none flex h-screen w-screen select-none flex-col items-center gap-2 px-6",
        bottom ? "justify-end pb-6" : "justify-start pt-6",
      ].join(" ")}
      style={{ background: "transparent" }}
    >
      {showIdleChip && <IdleChip />}
      {showPausedChip && <PausedChip />}
      {lines.map((l, i) => {
        const age = now - l.ts;
        // 临近淡出的最后 2.5 秒降到半透明，提示即将消失（fadeSeconds<=0 则不淡出）
        const dim =
          fadeSeconds > 0 && age > fadeSeconds * 1000 - 2500 ? 0.45 : 1;
        return (
          <div
            key={`${l.ts}-${i}`}
            className="animate-fade max-w-[90%] rounded-[11px] px-4 py-1.5 text-left leading-snug"
            style={{
              fontSize,
              fontWeight: 500,
              color: "#fff",
              background: `rgba(0,0,0,${opacity})`,
              backdropFilter: "blur(8px)",
              WebkitBackdropFilter: "blur(8px)",
              textShadow: "0 1px 3px rgba(0,0,0,.95), 0 0 1px rgba(0,0,0,.9)",
              opacity: dim,
              transition: "opacity .4s ease",
            }}
          >
            {l.text}
          </div>
        );
      })}
    </div>
  );
}

/** 监听中·无人说话：呼吸绿点 + 文案，避免用户误以为功能失效 */
function IdleChip() {
  return (
    <div
      className="flex items-center gap-2 rounded-full px-4 py-1.5 text-sm"
      style={{
        background: "rgba(0,0,0,.5)",
        backdropFilter: "blur(8px)",
        WebkitBackdropFilter: "blur(8px)",
        color: "rgba(255,255,255,.82)",
        letterSpacing: ".5px",
      }}
    >
      <span
        className="h-2 w-2 animate-pulse rounded-full"
        style={{ background: "#6ee7a0" }}
      />
      正在监听…
    </div>
  );
}

/** 已暂停：琥珀色点 + 文案 */
function PausedChip() {
  return (
    <div
      className="flex items-center gap-2 rounded-full px-4 py-1.5 text-sm"
      style={{
        background: "rgba(0,0,0,.5)",
        backdropFilter: "blur(8px)",
        WebkitBackdropFilter: "blur(8px)",
        color: "rgba(255,255,255,.82)",
        letterSpacing: ".5px",
      }}
    >
      <span
        className="h-2 w-2 rounded-full"
        style={{ background: "#e0a13a" }}
      />
      已暂停 · 按快捷键恢复
    </div>
  );
}
