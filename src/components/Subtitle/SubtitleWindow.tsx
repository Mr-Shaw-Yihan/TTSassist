// 字幕浮窗（subtitle_window）：透明 / 置顶 / 免焦点 / 鼠标穿透的纯展示叠加层。
// 只把后端 asr:subtitle 推来的字幕按设置渲染出来，所有控制都在主窗「字幕」页里。
// 高对比配色（白字 + 描边 + 黑底）固定，不随皮肤变化，保证游戏画面上始终可读。

import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTauriListen } from "../../hooks/useTauriListen";
import { getSettings, subtitleStatus } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import type { SubtitleLine } from "../../types";

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

  // 挂载：拉设置 + 初始状态；整窗设为鼠标穿透（纯叠加，绝不挡游戏点击）
  useEffect(() => {
    getSettings().then(setSettings).catch(() => {});
    subtitleStatus()
      .then((s) => {
        setRunning(s.running);
        setPaused(s.paused);
      })
      .catch(() => {});
    void getCurrentWindow().setIgnoreCursorEvents(true).catch(() => {});
  }, [setSettings]);

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

  // 新字幕：追加并截断到 maxLines（旧在前、新在后 → 底部为最新，字幕自下往上刷新）
  useTauriListen<SubtitleLine>(
    "asr:subtitle",
    (l) => {
      setLines((prev) => {
        const next = [...prev, l];
        return next.length > maxLines ? next.slice(next.length - maxLines) : next;
      });
    },
    [maxLines],
  );

  // 会话启停：停止时清空画面（下次开始重新计数）
  useTauriListen<{ running: boolean; paused: boolean }>(
    "subtitle:state",
    (p) => {
      setRunning(p.running);
      setPaused(p.paused);
      if (!p.running) setLines([]);
    },
    [],
  );
  // 暂停态切换
  useTauriListen<{ paused: boolean }>(
    "subtitle:paused",
    (p) => setPaused(p.paused),
    [],
  );
  // 设置变化（主窗改的）：重读换肤/换尺寸/换位置
  useTauriListen("settings:changed", () => {
    getSettings().then(setSettings).catch(() => {});
  }, []);

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
            className="animate-fade max-w-[92%] rounded-[11px] px-4 py-1.5 text-center leading-snug"
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

/** 已暂停：朱印色点 + 文案 */
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
