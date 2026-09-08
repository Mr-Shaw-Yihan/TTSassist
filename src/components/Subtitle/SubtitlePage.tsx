// 字幕管理页（侧边栏「📺 字幕」入口，仅装了 ASR 插件时可见）。
// 职责：配置监听源（进程 / 识别引擎 / 语言）+ VAD 灵敏度 + 暂停快捷键 + 浮窗外观 + 历史回看，
// 并统管 subtitle_window 的显隐 / 定位 / 鼠标穿透（浮窗自身是纯展示叠加层，不含控件）。
// 采「安墨」观感：卡片 + 琥珀竖条标题 + 分段 / 滑块 / 步进控件，全部即时写入 settings。

import { useCallback, useEffect, useRef, useState } from "react";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { primaryMonitor, PhysicalPosition } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { HotkeyRecorder } from "../Settings/HotkeyRecorder";
import {
  listAudioProcesses,
  listAsrPlugins,
  startAudioListener,
  stopAudioListener,
  subtitleStatus,
  setSubtitlePaused,
  getSubtitleSessions,
  exportSubtitleHistory,
  setSubtitlePauseHotkey,
} from "../../services/invoke";
import type {
  AudioProcess,
  AsrPluginInfo,
  SubtitleSession,
  SubtitleStatus,
} from "../../types";

const SUB_WIN_LABEL = "subtitle_window";

/** 秒 → m:ss */
function fmtElapsed(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

/** ms 时间戳 → HH:MM:SS（本地） */
function fmtClock(ts: number, withDate = false): string {
  const d = new Date(ts);
  const p = (n: number) => String(n).padStart(2, "0");
  const t = `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  return withDate ? `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${t}` : t;
}

export function SubtitlePage() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  const [status, setStatus] = useState<SubtitleStatus>({ running: false, paused: false });
  const [error, setError] = useState<string | null>(null);
  // 本次会话计数（前端累计 asr:subtitle 事件 + 计时，仅用于状态条展示）
  const [lineCount, setLineCount] = useState(0);
  const [elapsed, setElapsed] = useState(0);
  const startTsRef = useRef<number | null>(null);

  // 监听源
  const [procs, setProcs] = useState<AudioProcess[]>([]);
  const [procLoading, setProcLoading] = useState(false);
  const [asrPlugins, setAsrPlugins] = useState<AsrPluginInfo[]>([]);
  // 历史
  const [sessions, setSessions] = useState<SubtitleSession[]>([]);

  const position = settings?.subtitle_position ?? "bottom";
  const effectivePlugin = settings?.subtitle_asr_plugin || settings?.asr_plugin || "";

  // ── 数据加载 ────────────────────────────────
  const refreshProcs = useCallback(async () => {
    setProcLoading(true);
    setError(null);
    try {
      setProcs(await listAudioProcesses());
    } catch (e) {
      setError(String(e));
    } finally {
      setProcLoading(false);
    }
  }, []);

  const loadHistory = useCallback(async () => {
    try {
      setSessions(await getSubtitleSessions());
    } catch {
      /* 历史读取失败静默 */
    }
  }, []);

  useEffect(() => {
    void refreshProcs();
    listAsrPlugins().then(setAsrPlugins).catch(() => {});
    subtitleStatus().then(setStatus).catch(() => {});
    void loadHistory();
  }, [refreshProcs, loadHistory]);

  // ── 事件同步 ────────────────────────────────
  useTauriListen("asr:subtitle", () => setLineCount((c) => c + 1), []);
  useTauriListen<{ running: boolean; paused: boolean }>("subtitle:state", (p) => {
    setStatus(p);
    if (p.running) {
      startTsRef.current = Date.now();
      setLineCount(0);
      setElapsed(0);
    } else {
      startTsRef.current = null;
      void loadHistory();
    }
  }, [loadHistory]);
  useTauriListen<{ paused: boolean }>("subtitle:paused", (p) => {
    setStatus((s) => ({ ...s, paused: p.paused }));
  }, []);

  // 运行中计时
  useEffect(() => {
    if (!status.running) return;
    const id = window.setInterval(() => {
      if (startTsRef.current) setElapsed(Math.floor((Date.now() - startTsRef.current) / 1000));
    }, 1000);
    return () => window.clearInterval(id);
  }, [status.running]);

  // ── 浮窗显隐 / 定位 / 穿透 ───────────────────
  const getSubWin = useCallback(async () => {
    try {
      return await WebviewWindow.getByLabel(SUB_WIN_LABEL);
    } catch {
      return null;
    }
  }, []);

  /** 定位到底部 / 顶部居中（物理像素，避开任务栏用 workArea） */
  const positionSubWin = useCallback(
    async (w: WebviewWindow) => {
      const mon = await primaryMonitor().catch(() => null);
      const size = await w.innerSize().catch(() => null);
      if (!mon || !size) return;
      const wa = mon.workArea;
      const scale = mon.scaleFactor || 1;
      const margin = Math.round(24 * scale);
      const x = wa.position.x + Math.round((wa.size.width - size.width) / 2);
      const y =
        position === "top"
          ? wa.position.y + margin
          : wa.position.y + wa.size.height - size.height - margin;
      await w.setPosition(new PhysicalPosition(x, y)).catch(() => {});
    },
    [position],
  );

  const showSubWin = useCallback(async () => {
    const w = await getSubWin();
    if (!w) return;
    await positionSubWin(w);
    // 纯叠加层：始终鼠标穿透，绝不挡游戏/其他程序点击
    await w.setIgnoreCursorEvents(true).catch(() => {});
    await w.show().catch(() => {});
  }, [getSubWin, positionSubWin]);

  const hideSubWin = useCallback(async () => {
    const w = await getSubWin();
    await w?.hide().catch(() => {});
  }, [getSubWin]);

  // ── 操作 ────────────────────────────────────
  async function handleStart() {
    setError(null);
    if (!effectivePlugin) {
      setError("尚未选择可用的识别引擎，请先在上方选择或到「插件 / 语音」页安装 ASR 插件");
      return;
    }
    try {
      await startAudioListener();
      // 后端会广播 subtitle:state 刷新状态；这里负责把浮窗拉到前台就位
      await showSubWin();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleStop() {
    setError(null);
    try {
      await stopAudioListener();
      await hideSubWin();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleTogglePause() {
    try {
      const paused = await setSubtitlePaused(!status.paused);
      setStatus((s) => ({ ...s, paused }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handlePreview() {
    await showSubWin();
    await emit("asr:subtitle", { text: "这是字幕浮窗的显示效果预览", ts: Date.now() }).catch(
      () => {},
    );
  }

  async function handleExport() {
    try {
      const path = await save({
        title: "导出字幕历史",
        defaultPath: "subtitles.txt",
        filters: [{ name: "文本文件", extensions: ["txt"] }],
      });
      if (!path) return;
      await exportSubtitleHistory(path);
      window.alert(`已导出到：\n${path}`);
    } catch (e) {
      setError(String(e));
    }
  }

  // ── 派生：可选识别引擎 + 语言 ───────────────
  const loadedAsr = asrPlugins.filter((p) => p.loaded);
  const langPluginId = settings?.subtitle_asr_plugin || settings?.asr_plugin || "";
  const langSource = asrPlugins.find((p) => p.id === langPluginId);
  const langOptions: { code: string; label: string }[] = (() => {
    try {
      const arr = JSON.parse(langSource?.languages ?? "[]");
      return Array.isArray(arr) ? arr : [];
    } catch {
      return [];
    }
  })();
  const langList =
    langOptions.length > 0
      ? [{ code: "auto", label: "自动检测" }, ...langOptions]
      : [
          { code: "auto", label: "自动检测" },
          { code: "zh", label: "中文" },
          { code: "en", label: "English" },
        ];

  // 目标进程（按 exe 名记忆，展示友好名）
  const target = settings?.subtitle_target_process ?? "";

  return (
    <div className="scrollbar-thin h-full overflow-y-auto bg-[var(--paper)] px-5 py-5">
      {/* 页头 */}
      <div className="mb-4">
        <h1 className="font-display text-[22px] font-bold tracking-wide text-[var(--ink-900)]">字幕</h1>
        <p className="mt-1 text-xs text-[var(--ink-500)]">监听程序的声音，实时在屏幕上生成字幕</p>
      </div>

      {error && (
        <div className="mb-3 rounded-xl border border-[var(--seal)]/30 bg-[var(--seal)]/10 px-3.5 py-2.5 text-[12px] leading-relaxed text-[var(--seal)]">
          ✗ {error}
        </div>
      )}

      {/* 状态条 */}
      <div
        className={[
          "mb-4 flex items-center gap-3 rounded-xl border bg-[var(--paper-card)] px-4 py-3",
          status.running
            ? "border-[var(--amber-600)]/40 shadow-[0_8px_24px_rgba(26,24,22,0.06)]"
            : "border-[var(--ink-200)]",
        ].join(" ")}
      >
        <span
          className={
            "h-2.5 w-2.5 shrink-0 rounded-full " +
            (status.running ? (status.paused ? "bg-[var(--ink-300)]" : "is-playing bg-[var(--amber-600)]") : "bg-[var(--ink-300)]")
          }
        />
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-semibold text-[var(--ink-900)]">
            {status.running ? (status.paused ? "已暂停" : "监听中") : "已停止"}
          </div>
          <div className="mt-0.5 truncate text-[11.5px] text-[var(--ink-500)]">
            {status.running ? (
              <>
                {target ? `${target} · ` : ""}已转写 <b className="font-semibold text-[var(--amber-600)]">{lineCount}</b> 句 · 本次{" "}
                <b className="font-semibold text-[var(--amber-600)]">{fmtElapsed(elapsed)}</b>
              </>
            ) : (
              "选择监听源后点「开始监听」"
            )}
          </div>
        </div>
        {status.running && (
          <button
            onClick={handleTogglePause}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-1.5 text-xs font-medium text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
          >
            {status.paused ? "恢复" : "暂停"}
          </button>
        )}
        {status.running ? (
          <button
            onClick={handleStop}
            className="flex shrink-0 items-center gap-1.5 rounded-lg bg-[var(--seal)] px-3.5 py-1.5 text-xs font-semibold text-white transition-opacity hover:opacity-90"
          >
            <span className="text-[10px]">■</span> 停止监听
          </button>
        ) : (
          <button
            onClick={handleStart}
            disabled={!effectivePlugin}
            className="btn-tex flex shrink-0 items-center gap-1.5 rounded-lg px-3.5 py-1.5 text-xs font-semibold transition-opacity hover:opacity-95 disabled:cursor-not-allowed"
          >
            <span className="text-[10px]">▶</span> 开始监听
          </button>
        )}
      </div>

      {/* 监听源 */}
      <Card title="监听源">
        <Row label="监听进程" first>
          <div className="flex items-center gap-2">
            <select
              value={target}
              onChange={(e) => patch("subtitle_target_process", e.target.value)}
              className="min-w-0 flex-1 rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
            >
              <option value="">未指定（监听系统全部声音）</option>
              {procs.map((p) => (
                <option key={p.pid} value={p.name}>
                  {p.display_name}（{p.name}）{p.is_active ? "" : " · 空闲"}
                </option>
              ))}
            </select>
            <button
              onClick={refreshProcs}
              disabled={procLoading}
              title="刷新进程列表"
              className="flex h-[38px] w-[38px] shrink-0 items-center justify-center rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
            >
              <span className={procLoading ? "inline-block animate-spin" : ""}>⟳</span>
            </button>
          </div>
          <Hint>只有正在发出声音的进程才会被识别到 —— 先让目标程序出声，再点 ⟳ 刷新。此处仅作标注，实际监听的是系统混音。</Hint>
        </Row>
        <Row label="识别引擎">
          <select
            value={settings?.subtitle_asr_plugin ?? ""}
            onChange={(e) => patch("subtitle_asr_plugin", e.target.value)}
            className="w-full rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
          >
            <option value="">复用语音输入的引擎{settings?.asr_plugin ? `（${loadedAsr.find((p) => p.id === settings.asr_plugin)?.name ?? settings.asr_plugin}）` : ""}</option>
            {loadedAsr.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} v{p.version}
              </option>
            ))}
          </select>
          {loadedAsr.length === 0 && (
            <Hint>暂无可用识别引擎，请先在「插件」页安装 ASR 插件（如 MiMo ASR）。</Hint>
          )}
        </Row>
        <Row label="识别语言">
          <select
            value={settings?.subtitle_language ?? "auto"}
            onChange={(e) => patch("subtitle_language", e.target.value)}
            className="w-full rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
          >
            {langList.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
          <Hint>明确指定语言可提升识别准确率与速度。</Hint>
        </Row>
      </Card>

      {/* 语音检测（VAD） */}
      <Card title="语音检测" opt="VAD · 省钱核心">
        <Row label="灵敏度" first>
          <Segmented
            value={settings?.subtitle_vad_sensitivity ?? "medium"}
            onChange={(v) => patch("subtitle_vad_sensitivity", v)}
            options={[
              { value: "low", label: "低" },
              { value: "medium", label: "中" },
              { value: "high", label: "高" },
            ]}
          />
          <Hint>出现大量无意义字幕（游戏音效误触发）时，调低灵敏度；漏掉轻声说话时，调高。</Hint>
        </Row>
        <InfoStrip>
          <b>省钱机制：</b>绝不把音频持续送识别 —— 只有 VAD 检出的<b>真实语音</b>才转写，BGM、音效、静默全部丢弃、不计费。
        </InfoStrip>
      </Card>

      {/* 暂停快捷键 */}
      <Card title="暂停 / 恢复快捷键">
        <div className="pt-1">
          <HotkeyRecorder
            value={settings?.subtitle_pause_hotkey ?? "Alt+M"}
            onApply={(accel) => setSubtitlePauseHotkey(accel)}
          />
          <Hint>
            队伍语音出现敏感内容或想安静时随时按下暂停。保存时自动检测冲突（含被其他软件占用的键），冲突则不保存并提示。
          </Hint>
        </div>
      </Card>

      {/* 字幕浮窗 */}
      <Card title="字幕浮窗">
        <Row label="位置" first>
          <Segmented
            value={position}
            onChange={(v) => patch("subtitle_position", v)}
            options={[
              { value: "bottom", label: "底部居中" },
              { value: "top", label: "顶部" },
            ]}
          />
        </Row>
        <Row label="字号">
          <Slider
            value={settings?.subtitle_font_size ?? 18}
            min={14}
            max={32}
            step={1}
            display={`${settings?.subtitle_font_size ?? 18}px`}
            onChange={(v) => patch("subtitle_font_size", v)}
          />
        </Row>
        <Row label="底板透明度">
          <Slider
            value={Math.round((settings?.subtitle_opacity ?? 0.6) * 100)}
            min={20}
            max={90}
            step={5}
            display={`${Math.round((settings?.subtitle_opacity ?? 0.6) * 100)}%`}
            onChange={(v) => patch("subtitle_opacity", v / 100)}
          />
        </Row>
        <Row label="同屏条数">
          <Stepper
            value={settings?.subtitle_max_lines ?? 3}
            min={1}
            max={6}
            onChange={(v) => patch("subtitle_max_lines", v)}
          />
        </Row>
        <Row label="自动淡出">
          <Slider
            value={settings?.subtitle_fade_seconds ?? 15}
            min={0}
            max={60}
            step={1}
            display={
              (settings?.subtitle_fade_seconds ?? 15) === 0 ? "不淡出" : `${settings?.subtitle_fade_seconds ?? 15}秒`
            }
            onChange={(v) => patch("subtitle_fade_seconds", v)}
          />
        </Row>
        <div className="mt-3 flex items-center gap-2">
          <button
            onClick={handlePreview}
            className="btn-tex inline-flex items-center gap-1.5 rounded-[10px] px-3.5 py-2 text-xs font-semibold transition-opacity hover:opacity-95"
          >
            预览浮窗
          </button>
          <button
            onClick={hideSubWin}
            className="inline-flex items-center gap-1.5 rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-2 text-xs text-[var(--ink-700)] transition-colors hover:bg-[var(--ink-100)]"
          >
            隐藏浮窗
          </button>
          <span className="text-[11px] text-[var(--ink-300)]">浮窗始终高对比配色，不随主题变化</span>
        </div>
        <InfoStrip tone="warn">
          浮窗默认<b>鼠标穿透</b>，不会挡住游戏与程序点击。若游戏中看不到字幕，请把游戏改为<b>无边框窗口（Borderless）</b>模式 —— 独占全屏会遮挡置顶窗口。
        </InfoStrip>
      </Card>

      {/* 会话记录 */}
      <Card
        title="历史会话"
        right={
          <button
            onClick={handleExport}
            disabled={sessions.length === 0}
            className="inline-flex items-center gap-1.5 rounded-[10px] border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-1.5 text-xs text-[var(--ink-700)] transition-colors hover:bg-[var(--ink-100)] disabled:cursor-not-allowed disabled:opacity-40"
          >
            ↓ 导出 txt
          </button>
        }
      >
        {sessions.length === 0 ? (
          <p className="py-2 text-center text-[11.5px] text-[var(--ink-300)]">暂无历史记录。停止一次监听后，本轮字幕会存为一条会话。</p>
        ) : (
          <div className="space-y-2.5">
            {sessions.map((s, i) => (
              <div key={i} className="rounded-[10px] border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3 py-2.5">
                <div className="mb-1.5 flex items-center gap-2 text-[11px] text-[var(--ink-500)]">
                  <span className="font-mono">{fmtClock(s.started_ts, true)}</span>
                  <span className="text-[var(--ink-300)]">·</span>
                  <span>{s.lines.length} 句</span>
                  {s.process && (
                    <>
                      <span className="text-[var(--ink-300)]">·</span>
                      <span className="truncate">{s.process}</span>
                    </>
                  )}
                </div>
                <div className="space-y-0.5">
                  {s.lines.slice(0, 6).map((l, j) => (
                    <div key={j} className="flex gap-2 text-[12px] leading-relaxed">
                      <span className="shrink-0 font-mono text-[10.5px] text-[var(--ink-300)]">{fmtClock(l.ts)}</span>
                      <span className="min-w-0 flex-1 text-[var(--ink-700)]">{l.text}</span>
                    </div>
                  ))}
                  {s.lines.length > 6 && (
                    <div className="pl-[62px] text-[10.5px] text-[var(--ink-300)]">… 共 {s.lines.length} 句</div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
        <p className="mt-3 text-[10.5px] leading-relaxed text-[var(--ink-300)]">
          自动保留最近 5 次会话 · 仅存本地不上传 · 卸载即彻底删除。
        </p>
      </Card>

      <InfoStrip>
        字幕通常有 <b>2–5 秒延迟</b>（网络 + 识别耗时），属正常现象，不是功能失效。
      </InfoStrip>
    </div>
  );
}

// ── 局部控件（贴「安墨」观感，页内私有） ────────────────────────

function Card({
  title,
  opt,
  right,
  children,
}: {
  title: React.ReactNode;
  opt?: React.ReactNode;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-3.5 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3.5">
      <div className="mb-2 flex items-center gap-2">
        <span className="h-3.5 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
        <h3 className="font-display text-[13px] font-semibold tracking-wide text-[var(--ink-900)]">{title}</h3>
        {opt && <span className="text-[10.5px] text-[var(--ink-300)]">{opt}</span>}
        {right && <div className="ml-auto">{right}</div>}
      </div>
      <div>{children}</div>
    </div>
  );
}

function Row({ label, first, children }: { label: string; first?: boolean; children: React.ReactNode }) {
  return (
    <div className={"flex items-start gap-3 py-2" + (first ? "" : " border-t border-[var(--ink-200)]/60")}>
      <div className="w-[74px] shrink-0 pt-1.5 text-[12.5px] text-[var(--ink-700)]">{label}</div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

function Hint({ children }: { children: React.ReactNode }) {
  return <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">{children}</p>;
}

function Segmented({
  value,
  options,
  onChange,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <div className="inline-flex gap-0.5 rounded-[10px] bg-[var(--ink-100)] p-[3px]">
      {options.map((o) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={
            "rounded-lg px-3.5 py-1.5 text-xs transition-colors " +
            (value === o.value
              ? "bg-[var(--paper-card)] font-semibold text-[var(--amber-600)] shadow-[0_1px_3px_rgba(0,0,0,0.08)]"
              : "text-[var(--ink-500)] hover:text-[var(--ink-700)]")
          }
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Slider({
  value,
  min,
  max,
  step,
  display,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="h-1.5 flex-1 cursor-pointer"
        style={{ accentColor: "var(--amber-600)" }}
      />
      <span className="w-12 shrink-0 text-right text-xs font-semibold text-[var(--ink-700)]">{display}</span>
    </div>
  );
}

function Stepper({
  value,
  min,
  max,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="inline-flex items-center overflow-hidden rounded-lg border border-[var(--ink-200)]">
      <button
        onClick={() => onChange(Math.max(min, value - 1))}
        className="h-8 w-8 bg-[var(--paper)] text-base text-[var(--ink-500)] transition-colors hover:text-[var(--ink-900)]"
      >
        −
      </button>
      <span className="w-10 bg-[var(--paper-card)] text-center text-[13px] font-semibold text-[var(--ink-900)]">{value}</span>
      <button
        onClick={() => onChange(Math.min(max, value + 1))}
        className="h-8 w-8 bg-[var(--paper)] text-base text-[var(--ink-500)] transition-colors hover:text-[var(--ink-900)]"
      >
        +
      </button>
    </div>
  );
}

function InfoStrip({ tone = "info", children }: { tone?: "info" | "warn"; children: React.ReactNode }) {
  const warn = tone === "warn";
  return (
    <div
      className={
        "mt-3 flex items-start gap-2 rounded-[10px] border px-3 py-2.5 text-[11.5px] leading-relaxed " +
        (warn
          ? "border-[var(--seal)]/25 bg-[var(--seal)]/[.08] text-[var(--ink-700)]"
          : "border-[var(--amber-600)]/25 bg-[var(--amber-600)]/[.09] text-[var(--ink-700)]")
      }
    >
      <span className={"mt-px shrink-0 font-semibold " + (warn ? "text-[var(--seal)]" : "text-[var(--amber-600)]")}>
        {warn ? "⚠" : "ⓘ"}
      </span>
      <div>{children}</div>
    </div>
  );
}
