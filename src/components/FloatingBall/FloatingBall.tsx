// 悬浮球：常驻置顶小球，不依赖全局快捷键的启动方式（游戏内可用，无边框模式下置顶显示）。
//
// 交互（按用户规格）：
// - 左键按下直接拖拽（移动超过阈值才判定为拖拽，未超过则释放时视为单击）——
//   与常见小窗口一致的拖拽手感；手动 pointer capture + setPosition 实现
//   （Windows 透明/免焦点窗口上 startDragging 不可靠）
// - 单击：展开/收起快速输入浮窗（与「呼出浮窗」快捷键同一后端逻辑）
// - 右键：菜单 —— 打开主界面 / 开关发送到麦克风 / 播放最近一条消息 / 关闭悬浮球
//   （用 pointerdown button=2 触发——透明窗口上 contextmenu 事件可能不达）
// - 贴边：拖到屏幕左/右边缘附近释放后 0.5 秒自动滑出 2/3 到屏幕外（减少遮挡），
//   鼠标悬停滑出恢复，离开后重新计时；贴边位置不落盘（下次启动全量回到原位置）。
//   拖拽时小球本体钳制在屏幕内（不得超出边缘），贴边动画是唯一允许出屏的移动
//
// 窗口平时为 ballPx×ballPx 只露小球；菜单展开时把窗口临时放大容纳菜单，关闭后缩回。
// 小球锚定在窗口左上角，菜单向右下展开，因此开菜单不会挪动小球位置。
// 「播放最近一条消息」直接 emit playback:play-last（与快捷键同事件，主窗统一处理）。

import { useEffect, useRef, useState } from "react";
import { getCurrentWindow, currentMonitor, LogicalSize, PhysicalPosition } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getSettings, toggleQuickInput, toggleMicSend, setFloatingBallEnabled, saveFloatingBallPos, startOutsideClickWatch, stopOutsideClickWatch } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";

/** 直径安全范围（与后端 storage/types.rs 的 FLOATING_BALL_MIN/MAX 保持一致） */
const BALL_MIN = 40;
const BALL_MAX = 96;
const BALL_DEFAULT = 56;
/** 三个固定档位（与设置页三段控件一致）：小 / 标准（初始）/ 大 */
const BALL_TIERS = [44, 56, 72];
/** 把任意尺寸归一到最近档位（存量滑块时代的值也能对齐，避免三档 UI 无选中态） */
function snapToTier(v: number): number {
  const clamped = Math.min(BALL_MAX, Math.max(BALL_MIN, v));
  return BALL_TIERS.reduce((best, t) =>
    Math.abs(t - clamped) < Math.abs(best - clamped) ? t : best,
  );
}
/** 菜单面板尺寸（内容固定） */
const MENU_W = 216;
const MENU_H = 176;
/** 小球与菜单的间距 */
const MENU_GAP = 12;
/** 拖拽判定阈值（逻辑像素）：移动超过此距离才进入拖拽，否则释放视为单击 */
const DRAG_THRESHOLD = 4;
/** 贴边判定区（物理像素）：拖拽释放时小球距屏幕左/右边缘小于此值 → 准备贴边 */
const EDGE_ZONE = 48;
/** 释放后无操作多久自动贴边 */
const DOCK_DELAY_MS = 500;
/** 贴边/滑出动画时长 */
const DOCK_TWEEN_MS = 220;

function applyTheme(theme?: string) {
  document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
}

export function FloatingBall() {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [menuOpen, setMenuOpen] = useState(false);
  // 处于拖拽中：关掉按压缩放与过渡动画，避免拖拽时小球带延迟感
  const [isDragging, setIsDragging] = useState(false);
  // 本次按压仅用于关闭菜单（松开时不触发单击展开浮窗）
  const suppressClickRef = useRef(false);
  const moveSaveTimerRef = useRef<number | undefined>(undefined);
  // 拖拽会话：按下时的指针屏幕坐标 + 窗口物理位置 + 缩放系数；moved=是否已过拖拽阈值；
  // mon*=按下时所在显示器的物理边界（拖拽钳制用，获取失败为 null 则不钳制）
  const dragRef = useRef<{
    sx: number; sy: number; ox: number; oy: number; scale: number; moved: boolean;
    monLeft: number | null; monTop: number | null; monRight: number | null; monBottom: number | null;
  } | null>(null);
  // 拖拽平滑：最新值优先 + 最多一个 setPosition 在途。
  // 高频 pointermove 直接排队 IPC 会积压卡手（窗口永远在追旧光标），
  // 合流后只保留最新目标，在途完成后续发直到收敛到最终位置
  const dragBusyRef = useRef(false);
  const dragTargetRef = useRef<{ x: number; y: number } | null>(null);
  // ── 贴边（滑出 2/3）状态 ──
  // 已贴边的方向；非空时小球 2/3 在屏幕外
  const dockEdgeRef = useRef<"left" | "right" | null>(null);
  // 最近一次贴边倒计时的参数（边缘方向 + 全量可见 x + 显示器边界 + 小球物理直径）；
  // null = 不处于可贴边状态。悬停离开后重新计时时复用
  const dockParamsRef = useRef<{ edge: "left" | "right"; preDockX: number; monitorLeft: number; monitorRight: number; ballPhys: number } | null>(null);
  const dockTimerRef = useRef<number | undefined>(undefined);
  const dockAnimRafRef = useRef(0);
  // 贴边动画期间不把位置落盘（下次启动仍以全量可见位置还原）
  const dockAnimRef = useRef(false);

  // 小球直径（归一到最近的固定档位）
  const ballPx = snapToTier(settings?.floating_ball_size ?? BALL_DEFAULT);

  // 启动：加载设置 + 主题；窗口透明背景（悬浮球本体才有圆角和阴影）
  useEffect(() => {
    document.body.style.background = "transparent";
    (async () => {
      try {
        const s = await getSettings();
        setSettings(s);
        applyTheme(s.theme);
      } catch { /* 用默认 */ }
    })();
  }, [setSettings]);

  // 监听设置变化：同步 store（麦克风开关状态/尺寸）+ 换肤；被关闭时窗口由后端隐藏
  useTauriListen("settings:changed", async () => {
    try {
      const s = await getSettings();
      setSettings(s);
      applyTheme(s.theme);
    } catch { /* ignore */ }
  }, [setSettings]);

  // 菜单开合 / 尺寸变化 → 调整窗口尺寸（小球在左上角不动，菜单向下展开）
  useEffect(() => {
    const size = menuOpen
      ? new LogicalSize(Math.max(ballPx, MENU_W), ballPx + MENU_GAP + MENU_H)
      : new LogicalSize(ballPx, ballPx);
    void getCurrentWindow().setSize(size);
  }, [menuOpen, ballPx]);

  // 菜单展开期间的「窗口外点击」收回：免焦点窗口收不到桌面/其他程序的点击事件，
  // 由后端轮询检测（尺寸传展开后的逻辑尺寸，不依赖异步 setSize 完成）
  useEffect(() => {
    if (menuOpen) {
      void startOutsideClickWatch(Math.max(ballPx, MENU_W), ballPx + MENU_GAP + MENU_H).catch(() => {});
    } else {
      void stopOutsideClickWatch().catch(() => {});
    }
    return () => { void stopOutsideClickWatch().catch(() => {}); };
  }, [menuOpen, ballPx]);

  // 后端检测到窗口外点击 → 收菜单
  useTauriListen("floating_ball:outside-click", () => setMenuOpen(false), []);

  // 窗口移动（拖拽）→ 防抖保存位置（下次启动还原）；贴边动画的移动不落盘
  useEffect(() => {
    const win = getCurrentWindow();
    let un: (() => void) | null = null;
    (async () => {
      un = await win.onMoved(({ payload }) => {
        window.clearTimeout(moveSaveTimerRef.current);
        moveSaveTimerRef.current = window.setTimeout(() => {
          if (dockAnimRef.current) return; // 贴边位置不保存
          void saveFloatingBallPos(payload.x, payload.y).catch(() => {});
        }, 400);
      });
    })();
    return () => { un?.(); window.clearTimeout(moveSaveTimerRef.current); };
  }, []);

  // 销毁时清理贴边定时器与动画
  useEffect(() => () => {
    window.clearTimeout(dockTimerRef.current);
    cancelAnimationFrame(dockAnimRafRef.current);
  }, []);

  // ── 贴边（滑出 2/3）：拖到屏幕左/右边缘附近释放 → 0.5 秒无操作自动隐藏，减少遮挡 ──

  function cancelDockTimer() {
    window.clearTimeout(dockTimerRef.current);
    dockTimerRef.current = undefined;
  }

  /** 补间动画把窗口 x 滑到目标值（y 不动）；动画期间禁止位置落盘 */
  function tweenWindowX(to: number) {
    cancelAnimationFrame(dockAnimRafRef.current);
    dockAnimRef.current = true;
    void (async () => {
      try {
        const pos = await getCurrentWindow().outerPosition();
        const fromX = pos.x;
        const y = pos.y;
        const t0 = performance.now();
        const step = () => {
          const k = Math.min(1, (performance.now() - t0) / DOCK_TWEEN_MS);
          const ease = 1 - Math.pow(1 - k, 3); // ease-out
          const x = Math.round(fromX + (to - fromX) * ease);
          void getCurrentWindow().setPosition(new PhysicalPosition(x, y)).catch(() => {});
          if (k < 1) {
            dockAnimRafRef.current = requestAnimationFrame(step);
          } else {
            dockAnimRef.current = false;
            // 清掉动画期间 onMoved 挂起的保存定时器（贴边位置不落盘）
            window.clearTimeout(moveSaveTimerRef.current);
          }
        };
        dockAnimRafRef.current = requestAnimationFrame(step);
      } catch { dockAnimRef.current = false; }
    })();
  }

  /** 武装贴边倒计时：DOCK_DELAY_MS 内无新操作则滑入半隐藏 */
  function armDock(
    edge: "left" | "right",
    preDockX: number,
    monitorLeft: number,
    monitorRight: number,
    ballPhys: number,
  ) {
    cancelDockTimer();
    dockParamsRef.current = { edge, preDockX, monitorLeft, monitorRight, ballPhys };
    dockTimerRef.current = window.setTimeout(() => {
      if (dockEdgeRef.current || dragRef.current) return;
      dockEdgeRef.current = edge;
      // 滑出 2/3 到屏幕外（留 1/3 可见，够悬停/点击）
      const hidden = Math.round((ballPhys * 2) / 3);
      tweenWindowX(edge === "left" ? monitorLeft - hidden : monitorRight - hidden);
    }, DOCK_DELAY_MS);
  }

  /** 拖拽结束后检查是否落在贴边区：是 → 武装倒计时；否 → 退出可贴边状态 */
  function maybeDock(x: number, scale: number) {
    void (async () => {
      try {
        const mon = await currentMonitor();
        if (!mon) { dockParamsRef.current = null; return; }
        const ballPhys = Math.round(ballPx * scale);
        const left = mon.position.x;
        const right = mon.position.x + mon.size.width;
        if (x - left <= EDGE_ZONE) {
          armDock("left", x, left, right, ballPhys);
        } else if (right - (x + ballPhys) <= EDGE_ZONE) {
          armDock("right", x, left, right, ballPhys);
        } else {
          dockParamsRef.current = null;
          dockEdgeRef.current = null;
        }
      } catch { /* 贴边是体验增强，失败静默 */ }
    })();
  }

  /** 悬停/按下已贴边的小球 → 滑回全量可见位置 */
  function revealIfDocked() {
    if (!dockEdgeRef.current) return;
    dockEdgeRef.current = null;
    cancelDockTimer();
    const p = dockParamsRef.current;
    if (p) tweenWindowX(p.preDockX);
  }

  function onPointerDown(e: React.PointerEvent<HTMLButtonElement>) {
    if (e.button === 2) {
      // 右键：切换菜单（透明窗口上 contextmenu 事件不可靠，用 pointerdown 触发）
      e.preventDefault();
      e.stopPropagation();
      setMenuOpen((v) => !v);
      return;
    }
    if (e.button !== 0) return;
    e.stopPropagation();
    if (menuOpen) {
      // 菜单开着时左键小球：仅收起菜单，不展开浮窗
      setMenuOpen(false);
      suppressClickRef.current = true;
      return;
    }
    suppressClickRef.current = false;
    cancelDockTimer();
    revealIfDocked();
    // React 合成事件的 currentTarget 在处理器返回后会置空，
    // 异步里要用，必须同步捕获元素引用与坐标
    const el = e.currentTarget;
    const sx = e.screenX;
    const sy = e.screenY;
    const pid = e.pointerId;
    // 直接拖拽：按下即建立会话；移动超阈值才进入拖拽模式，
    // 未超过则释放时视为单击（与普通小窗口一致的拖拽手感）
    void (async () => {
      try {
        const win = getCurrentWindow();
        const pos = await win.outerPosition();
        const scale = await win.scaleFactor();
        // 记录按下时所在显示器的边界：拖拽期间钳制用（小球本体不能超过屏幕边缘）
        const mon = await currentMonitor().catch(() => null);
        dragRef.current = {
          sx, sy, ox: pos.x, oy: pos.y, scale, moved: false,
          monLeft: mon ? mon.position.x : null,
          monTop: mon ? mon.position.y : null,
          monRight: mon ? mon.position.x + mon.size.width : null,
          monBottom: mon ? mon.position.y + mon.size.height : null,
        };
        el.setPointerCapture(pid);
      } catch { /* 拖拽准备失败：本次按压退化为纯点击 */ }
    })();
  }

  /** 把最新拖拽目标发给窗口；在途时不发，完成后若又有新目标则续发 */
  function flushDragPos() {
    if (dragBusyRef.current || !dragTargetRef.current) return;
    const t = dragTargetRef.current;
    dragTargetRef.current = null;
    dragBusyRef.current = true;
    getCurrentWindow()
      .setPosition(new PhysicalPosition(t.x, t.y))
      .catch(() => {})
      .finally(() => {
        dragBusyRef.current = false;
        flushDragPos();
      });
  }

  /** 把拖拽目标钳进按下时所在显示器内（小球本体不能超过屏幕边缘；贴边动画是唯一例外） */
  function clampToScreen(d: NonNullable<(typeof dragRef)["current"]>, x: number, y: number) {
    if (d.monLeft === null || d.monTop === null || d.monRight === null || d.monBottom === null) {
      return { x, y };
    }
    const ballPhys = Math.round(ballPx * d.scale);
    return {
      x: Math.min(Math.max(x, d.monLeft), Math.max(d.monLeft, d.monRight - ballPhys)),
      y: Math.min(Math.max(y, d.monTop), Math.max(d.monTop, d.monBottom - ballPhys)),
    };
  }

  function onPointerMove(e: React.PointerEvent) {
    const d = dragRef.current;
    if (!d) return;
    if (!d.moved) {
      // 单击 vs 拖拽：超过阈值才进入拖拽模式
      if (Math.hypot(e.screenX - d.sx, e.screenY - d.sy) < DRAG_THRESHOLD) return;
      d.moved = true;
      setIsDragging(true);
      dragBusyRef.current = false;
      dragTargetRef.current = null;
      // 拖拽中退出贴边状态，并停掉可能还在跑的贴边动画（避免两路 setPosition 打架）
      dockEdgeRef.current = null;
      cancelAnimationFrame(dockAnimRafRef.current);
      dockAnimRef.current = false;
    }
    const dx = (e.screenX - d.sx) * d.scale;
    const dy = (e.screenY - d.sy) * d.scale;
    // 只记最新目标（钳进屏幕内），发送频率由 IPC 完成速率自动限流（见 flushDragPos）
    dragTargetRef.current = clampToScreen(d, Math.round(d.ox + dx), Math.round(d.oy + dy));
    flushDragPos();
  }

  function onPointerUp(e: React.PointerEvent<HTMLButtonElement>) {
    if (e.button !== 0) return;
    const d = dragRef.current;
    dragRef.current = null;
    if (d?.moved) {
      // 拖拽结束：把最后一个目标补发到位并落盘。
      // 以请求的目标为准而非重读 outerPosition——避免与在途 setPosition 竞态读到旧位置
      setIsDragging(false);
      const target = dragTargetRef.current;
      dragTargetRef.current = null;
      if (target) {
        void getCurrentWindow()
          .setPosition(new PhysicalPosition(target.x, target.y))
          .catch(() => {});
        void saveFloatingBallPos(target.x, target.y).catch(() => {});
        maybeDock(target.x, d.scale);
      } else {
        void getCurrentWindow()
          .outerPosition()
          .then((p) => {
            void saveFloatingBallPos(p.x, p.y).catch(() => {});
            maybeDock(p.x, d.scale);
          })
          .catch(() => {});
      }
      return;
    }
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    // 单击（未超过拖拽阈值）→ 展开/收起快速输入浮窗
    void toggleQuickInput().catch(() => {});
  }

  function onPointerCancel() {
    dragRef.current = null;
    setIsDragging(false);
    dragTargetRef.current = null;
  }

  /** 悬停进入：已贴边则滑出 */
  function onBallPointerEnter() {
    revealIfDocked();
  }

  /** 悬停离开：处于可贴边状态则重新武装 1 秒倒计时（拖拽中有 pointer capture 不会误触发） */
  function onBallPointerLeave() {
    const p = dockParamsRef.current;
    if (p && !dockEdgeRef.current && !dragRef.current) {
      armDock(p.edge, p.preDockX, p.monitorLeft, p.monitorRight, p.ballPhys);
    }
  }

  const micOn =
    (settings?.mic_send_enabled ?? false) &&
    !!(settings?.mic_output_device && settings.mic_output_device.trim());

  return (
    <div
      className="relative h-screen w-screen select-none overflow-hidden bg-transparent"
      onPointerDown={() => { if (menuOpen) setMenuOpen(false); }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* 小球本体（窗口左上角锚定，直径跟随设置） */}
      <button
        type="button"
        aria-label="电子声带悬浮球"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onPointerEnter={onBallPointerEnter}
        onPointerLeave={onBallPointerLeave}
        onContextMenu={(e) => e.preventDefault()}
        className={[
          "flex touch-none items-center justify-center rounded-full",
          "bg-[var(--ink-900)]",
          "shadow-[0_6px_20px_rgba(26,24,22,0.45)] ring-1 ring-[var(--amber-500)]/70",
          // 拖拽中不带过渡动画（瞬移跟手）；平时保留按压缩放反馈
          isDragging ? "scale-95" : "transition-transform active:scale-95",
        ].join(" ")}
        style={{ width: ballPx, height: ballPx }}
      />

      {/* 右键菜单（展开窗口后在小球下方渲染） */}
      {menuOpen && (
        <div
          className="absolute left-2 w-52 rounded-2xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1.5 shadow-[0_16px_48px_rgba(26,24,22,0.35)]"
          style={{ top: ballPx + MENU_GAP }}
          onPointerDown={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
        >
          <MenuItem
            icon="⌂"
            label="打开主界面"
            onClick={() => { setMenuOpen(false); void invoke("show_main_window").catch(() => {}); }}
          />
          <MenuItem
            icon={micOn ? "🎙" : "🔇"}
            label={micOn ? "关闭发送到麦克风" : "开启发送到麦克风"}
            hint={settings?.mic_output_device ? undefined : "（未配置设备）"}
            onClick={() => { setMenuOpen(false); void toggleMicSend().catch(() => {}); }}
          />
          <MenuItem
            icon="▶"
            label="播放最近一条消息"
            onClick={() => { setMenuOpen(false); void emit("playback:play-last").catch(() => {}); }}
          />
          <div className="mx-2 my-1 border-t border-[var(--ink-100)]" />
          <MenuItem
            icon="✕"
            label="关闭悬浮球"
            danger
            onClick={() => { setMenuOpen(false); void setFloatingBallEnabled(false).catch(() => {}); }}
          />
        </div>
      )}
    </div>
  );
}

function MenuItem({
  icon,
  label,
  hint,
  danger,
  onClick,
}: {
  icon: string;
  label: string;
  hint?: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "flex w-full items-center gap-2.5 px-3.5 py-2 text-left text-[13px] transition-colors",
        danger
          ? "text-[var(--seal)] hover:bg-[var(--seal)]/10"
          : "text-[var(--ink-700)] hover:bg-[var(--ink-100)]/60 hover:text-[var(--ink-900)]",
      ].join(" ")}
    >
      <span className="w-4 text-center text-xs opacity-70">{icon}</span>
      <span className="flex-1">{label}</span>
      {hint && <span className="text-[10px] text-[var(--ink-300)]">{hint}</span>}
    </button>
  );
}
