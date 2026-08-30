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
import { getSettings, toggleQuickInput, toggleMicSend, setFloatingBallEnabled, saveFloatingBallPos, startOutsideClickWatch, stopOutsideClickWatch, isBootReady, startCursorWatch, stopCursorWatch, updateBallHitRect, setBallPassthroughOverride } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { createBallCharacter, type BallCharacter } from "./engine";
import { AudioRipples } from "./AudioRipples";

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
/** 动画画布 = 球径 × 1.5（球居中，四周透明留白给粒子/菜单，窗口尺寸永不变化） */
const CANVAS_SCALE = 1.5;
/** 贴边判定区（物理像素）：仅当球体实质靠到屏幕边缘（≈贴着）才贴边；
 *  画布靠边但球体未靠边不触发（画布有透明留白，球离边还有 offPhys） */
const EDGE_ZONE = 16;
/** 释放后无操作多久自动贴边 */
const DOCK_DELAY_MS = 500;
/** 贴边/滑出动画时长 */
const DOCK_TWEEN_MS = 220;

function applyTheme(theme?: string) {
  document.documentElement.setAttribute("data-theme", theme === "dark" ? "dark" : "light");
}

/** 皮肤应用：ink=墨黑体+奶油眼（默认）/ white=素白体+墨黑眼 */
function applySkinTo(c: BallCharacter, skin: string) {
  if (skin === "white") {
    c.setInk("#f7f4ec");
    c.setEyeColor("#1a1816");
  } else {
    c.setInk("#0a0a0a");
    c.setEyeColor(null);
  }
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
    offPhys: number;
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
  const dockParamsRef = useRef<{ edge: "left" | "right"; preDockX: number; monitorLeft: number; monitorRight: number; ballPhys: number; offPhys: number } | null>(null);
  const dockTimerRef = useRef<number | undefined>(undefined);
  const dockAnimRafRef = useRef(0);
  // 贴边动画期间不把位置落盘（下次启动仍以全量可见位置还原）
  const dockAnimRef = useRef(false);

  // 小球直径（归一到最近的固定档位）
  const ballPx = snapToTier(settings?.floating_ball_size ?? BALL_DEFAULT);
  // 画布尺寸与球在画布内的偏移（球居中）
  const canvas = Math.round(ballPx * CANVAS_SCALE);
  const off = Math.round((canvas - ballPx) / 2);

  // 角色引擎实例与 SVG 宿主
  const svgRef = useRef<SVGSVGElement | null>(null);
  const charRef = useRef<BallCharacter | null>(null);
  const errorRecoverTimerRef = useRef<number | undefined>(undefined);
  // 退场动画（收回时先演后隐）
  const [despawning, setDespawning] = useState(false);
  // boot 完成标记（决定球窗是否可见：enabled 或 boot 未完结）
  const [bootDone, setBootDone] = useState(false);
  // 音频播放中（波纹开关）
  const [playing, setPlaying] = useState(false);
  // 窗口物理位置/缩放缓存（光标屏幕坐标 → 客户区坐标换算用）
  const winPosRef = useRef<{ x: number; y: number } | null>(null);
  const scaleRef = useRef(1);
  const followActiveRef = useRef(false);
  // 菜单展开视为互动中：不武装/不触发贴边
  const menuOpenRef = useRef(false);
  // 主显示器边界缓存（左右侧判定用）
  const monRef = useRef<{ left: number; width: number } | null>(null);
  // 球在屏幕右半 → 角色水平镜像（眼睛朝向屏幕内，右侧贴边时不埋眼）
  const [mirrored, setMirrored] = useState(false);
  const mirroredRef = useRef(false);
  const canvasRef = useRef(0);
  // 位置变化计数（onMoved 触发交互区域/侧向重算）
  const [posTick, setPosTick] = useState(0);
  // 性能策略与球窗可见性（门控指针跟随/波纹/帧率）
  const perfMode = settings?.floating_ball_perf_mode ?? "standard";
  const ballVisible = (settings?.floating_ball_enabled ?? false) || !bootDone;
  canvasRef.current = canvas;
  mirroredRef.current = mirrored;
  menuOpenRef.current = menuOpen;

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

  // 还原位置专用事件：清除贴边状态（避免「还原后悬停又贴回」）。
  // 不走 settings:changed——换档尺寸变化也广播它，误清贴边会导致比例错乱
  useTauriListen("floating_ball:reset", () => {
    if (dragRef.current?.moved) return;
    cancelDockTimer();
    dockParamsRef.current = null;
    dockEdgeRef.current = null;
    cancelAnimationFrame(dockAnimRafRef.current);
    dockAnimRef.current = false;
  }, []);

  // 音频波纹开关（主窗/浮窗播放音频时发 va:play:*）
  useTauriListen("va:play:start", () => setPlaying(true), []);
  useTauriListen("va:play:stop", () => setPlaying(false), []);

  // 指针跟随：后端广播光标屏幕物理坐标 → 换算客户区坐标喂引擎视线（镜像时 x 翻转）
  useTauriListen<[number, number]>("floating_ball:cursor", (pt) => {
    if (!followActiveRef.current) return;
    const c = charRef.current;
    const wp = winPosRef.current;
    if (!c || !wp) return;
    const sc = scaleRef.current || 1;
    const x = (pt[0] - wp.x) / sc;
    c.setGazeTarget({ x: mirroredRef.current ? canvasRef.current - x : x, y: (pt[1] - wp.y) / sc });
  }, []);

  // 光标轮询启停：球可见即启动（穿透判定两档模式都需要）；
  // 仅标准模式广播光标事件供视线跟随
  useEffect(() => {
    followActiveRef.current = ballVisible && perfMode === "standard";
    if (ballVisible) {
      void startCursorWatch(perfMode === "standard").catch(() => {});
    } else {
      void stopCursorWatch().catch(() => {});
      charRef.current?.setGazeTarget(null);
    }
    return () => { void stopCursorWatch().catch(() => {}); };
  }, [ballVisible, perfMode]);

  // 同步交互区域（区域外光标穿透）+ 屏幕左右侧判定（右半镜像）
  useEffect(() => {
    const wp = winPosRef.current;
    const sc = scaleRef.current || 1;
    if (!wp) return;
    const offPhys = Math.round(off * sc);
    const ballPhys = Math.round(ballPx * sc);
    if (menuOpen) {
      // 菜单展开：球+菜单区域可交互
      void updateBallHitRect(
        wp.x,
        wp.y,
        Math.round(canvas * sc),
        Math.round((off + ballPx + MENU_GAP + MENU_H) * sc),
      ).catch(() => {});
    } else {
      // 平时：仅球体方块可交互，其余画布穿透
      void updateBallHitRect(wp.x + offPhys, wp.y + offPhys, ballPhys, ballPhys).catch(() => {});
    }
    const mon = monRef.current;
    if (mon) {
      setMirrored(wp.x + (canvas * sc) / 2 > mon.left + mon.width / 2);
    }
  }, [posTick, menuOpen, ballPx, canvas, off]);

  // 性能模式：引擎帧率封顶切换
  useEffect(() => {
    charRef.current?.setFpsCap(perfMode === "performance" ? 30 : 0);
  }, [perfMode]);

  // 皮肤切换即时生效
  const skin = settings?.floating_ball_skin ?? "ink";
  useEffect(() => {
    if (charRef.current) applySkinTo(charRef.current, skin);
  }, [skin]);

  // 窗口位置/缩放/显示器缓存初始化（光标换算 + 侧向判定用）
  useEffect(() => {
    void (async () => {
      try {
        const win = getCurrentWindow();
        winPosRef.current = await win.outerPosition();
        scaleRef.current = await win.scaleFactor();
        const mon = await currentMonitor();
        if (mon) monRef.current = { left: mon.position.x, width: mon.size.width };
        setPosTick((v) => v + 1);
      } catch { /* 忽略 */ }
    })();
  }, []);
  useEffect(() => {
    const edge = dockEdgeRef.current;
    const p = dockParamsRef.current;
    if (!p) return;
    if (!edge) {
      // 未贴边仅可贴边：旧尺寸的参数已无意义，清掉等下次拖拽重建
      dockParamsRef.current = null;
      return;
    }
    void (async () => {
      try {
        const win = getCurrentWindow();
        const pos = await win.outerPosition();
        const scale = await win.scaleFactor();
        const ballPhys = Math.round(ballPx * scale);
        const offPhys = Math.round(((canvas - ballPx) / 2) * scale);
        const hidden = Math.round((ballPhys * 2) / 3);
        const x =
          edge === "left"
            ? p.monitorLeft - hidden - offPhys
            : p.monitorRight + hidden - ballPhys - offPhys;
        p.ballPhys = ballPhys;
        p.offPhys = offPhys;
        void win.setPosition(new PhysicalPosition(x, pos.y)).catch(() => {});
      } catch { /* 忽略 */ }
    })();
  }, [ballPx, canvas]);

  // 菜单开合 / 尺寸变化 → 调整窗口尺寸（画布固定，菜单在画布下方展开）
  useEffect(() => {
    const size = menuOpen
      ? new LogicalSize(Math.max(canvas, MENU_W + 8), off + ballPx + MENU_GAP + MENU_H)
      : new LogicalSize(canvas, canvas);
    void getCurrentWindow().setSize(size);
  }, [menuOpen, ballPx, canvas, off]);

  // 菜单展开期间的「窗口外点击」收回：免焦点窗口收不到桌面/其他程序的点击事件，
  // 由后端轮询检测（尺寸传展开后的逻辑尺寸，不依赖异步 setSize 完成）
  useEffect(() => {
    if (menuOpen) {
      void startOutsideClickWatch(Math.max(canvas, MENU_W + 8), off + ballPx + MENU_GAP + MENU_H).catch(() => {});
    } else {
      void stopOutsideClickWatch().catch(() => {});
    }
    return () => { void stopOutsideClickWatch().catch(() => {}); };
  }, [menuOpen, ballPx, canvas, off]);

  // 菜单开合同步语义事件（渲染器监听 va:menu:* 切 suspicious/idle）
  useEffect(() => {
    void emit(menuOpen ? "va:menu:open" : "va:menu:close").catch(() => {});
  }, [menuOpen]);

  // 后端检测到窗口外点击 → 收菜单
  useTauriListen("floating_ball:outside-click", () => setMenuOpen(false), []);

  // ── 角色引擎：创建/销毁 + 主题同步 ──
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const ch = createBallCharacter(svg, {
      scheme: document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light",
    });
    charRef.current = ch;
    // 皮肤：ink=墨黑体+奶油眼 / white=素白体+墨黑眼
    applySkinTo(
      ch,
      useSettingsStore.getState().settings?.floating_ball_skin ?? "ink",
    );
    ch.setFpsCap(
      (useSettingsStore.getState().settings?.floating_ball_perf_mode ?? "standard") === "performance" ? 30 : 0,
    );
    // 启动演出：progress（boot 控制器收到 va:boot:ready 后切 waking）
    ch.setState("progress");
    return () => {
      charRef.current = null;
      window.clearTimeout(errorRecoverTimerRef.current);
      ch.destroy();
    };
  }, []);

  // ── boot 控制器：progress → max(实际 ready, 800ms) → waking → 收球开主窗 ──
  // 双保险：事件 + 主动查询（dev 慢加载时前端挂载可能晚于事件，事件会丢）
  const bootAtRef = useRef(performance.now());
  const bootDoneRef = useRef(false);
  function tryBoot() {
    if (bootDoneRef.current) return;
    void (async () => {
      let ready = false;
      try { ready = await isBootReady(); } catch { ready = false; }
      if (!ready || bootDoneRef.current) return;
      bootDoneRef.current = true;
      const wait = Math.max(0, 800 - (performance.now() - bootAtRef.current));
      window.setTimeout(() => {
        const c = charRef.current;
        c?.setState("waking");
        window.setTimeout(() => {
          setBootDone(true);
          void invoke("show_main_window").catch(() => {});
          if (useSettingsStore.getState().settings?.floating_ball_enabled ?? false) {
            c?.setState("idle"); // 上次关机时球在外 → 启动后保持在外
          } else {
            void getCurrentWindow().hide().catch(() => {}); // 收球入 logo
          }
        }, 800);
      }, wait);
    })();
  }
  useEffect(() => { tryBoot(); }, []);
  useTauriListen("va:boot:ready", () => tryBoot(), []);

  // 收回退场：收缩+淡出 700ms 后自隐（后端 1s 兜底强隐）
  useTauriListen("floating_ball:despawn", () => {
    setDespawning(true);
    window.setTimeout(() => {
      void getCurrentWindow()
        .hide()
        .catch(() => {})
        .then(() => setDespawning(false));
    }, 700);
  }, []);

  // 放出球（enabled false→true）→ waking 唤醒后回 idle
  const prevEnabledRef = useRef<boolean | null>(null);
  useEffect(() => {
    const en = settings?.floating_ball_enabled ?? false;
    const prev = prevEnabledRef.current;
    prevEnabledRef.current = en;
    if (prev === false && en) {
      const c = charRef.current;
      c?.setState("waking");
      window.setTimeout(() => c?.setState("idle"), 800);
    }
  }, [settings?.floating_ball_enabled]);

  // ── va:* 事件词表 → 角色状态（映射层；换原创素材只改这里与 engine/）──
  useTauriListen("va:drag:start", () => charRef.current?.setState("dragging"), []);
  useTauriListen("va:drag:end", () => charRef.current?.setState("idle"), []);
  useTauriListen("va:menu:open", () => charRef.current?.setState("suspicious"), []);
  useTauriListen("va:menu:close", () => charRef.current?.setState("idle"), []);
  useTauriListen("va:dock", () => charRef.current?.setState("bored"), []);
  useTauriListen("va:undock", () => charRef.current?.setState("idle"), []);
  useTauriListen("va:tts:done", () => charRef.current?.spinOnce(1), []);
  useTauriListen("va:asr:start", () => charRef.current?.setState("receiving"), []);
  useTauriListen("va:asr:end", () => charRef.current?.setState("idle"), []);
  useTauriListen("va:tts:error", () => {
    const c = charRef.current;
    if (!c) return;
    c.setState("alerting");
    window.clearTimeout(errorRecoverTimerRef.current);
    // 3 秒后 waking 回常态（报错互动恢复走主窗另行 emit，本期先用定时兜底）
    errorRecoverTimerRef.current = window.setTimeout(() => {
      c.setState("waking");
      window.setTimeout(() => c.setState("idle"), 800);
    }, 3000);
  }, []);

  // 窗口移动（拖拽）→ 防抖保存位置（下次启动还原）；贴边动画的移动不落盘
  useEffect(() => {
    const win = getCurrentWindow();
    let un: (() => void) | null = null;
    (async () => {
      un = await win.onMoved(({ payload }) => {
        winPosRef.current = { x: payload.x, y: payload.y };
        setPosTick((v) => v + 1);
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
    offPhys: number,
  ) {
    cancelDockTimer();
    dockParamsRef.current = { edge, preDockX, monitorLeft, monitorRight, ballPhys, offPhys };
    dockTimerRef.current = window.setTimeout(() => {
      if (dockEdgeRef.current || dragRef.current || menuOpenRef.current) return;
      dockEdgeRef.current = edge;
      void emit("va:dock").catch(() => {});
      // 滑出 2/3 到屏幕外（留 1/3 可见，够悬停/点击）；以球边计算（画布多 offPhys 留白）
      const hidden = Math.round((ballPhys * 2) / 3);
      tweenWindowX(
        edge === "left"
          ? monitorLeft - hidden - offPhys
          : monitorRight + hidden - ballPhys - offPhys,
      );
    }, DOCK_DELAY_MS);
  }

  /** 拖拽结束后检查是否落在贴边区：是 → 武装倒计时；否 → 退出可贴边状态 */
  function maybeDock(x: number, scale: number, offPhys: number) {
    void (async () => {
      try {
        const mon = await currentMonitor();
        if (!mon) { dockParamsRef.current = null; return; }
        const ballPhys = Math.round(ballPx * scale);
        const left = mon.position.x;
        const right = mon.position.x + mon.size.width;
        const ballLeft = x + offPhys;
        if (ballLeft - left <= EDGE_ZONE) {
          armDock("left", x, left, right, ballPhys, offPhys);
        } else if (right - (ballLeft + ballPhys) <= EDGE_ZONE) {
          armDock("right", x, left, right, ballPhys, offPhys);
        } else {
          dockParamsRef.current = null;
          dockEdgeRef.current = null;
        }
      } catch { /* 贴边是体验增强，失败静默 */ }
    })();
  }

  /** 悬停/按下已贴边的小球 → 滑回全量可见位置 */
  function revealIfDocked() {
    const edge = dockEdgeRef.current;
    const p = dockParamsRef.current;
    if (!edge || !p) return;
    dockEdgeRef.current = null;
    cancelDockTimer();
    void emit("va:undock").catch(() => {});
    // 滑出目标 = 球体完全可见且贴回同侧边缘：球在光标下原地展开，
    // 不横向跳开（旧逻辑弹回拖拽前位置，光标会落到透明画布上导致又缩回）
    const x =
      edge === "left"
        ? p.monitorLeft - p.offPhys
        : p.monitorRight - p.ballPhys - p.offPhys;
    tweenWindowX(x);
  }

  function onPointerDown(e: React.PointerEvent<HTMLButtonElement>) {
    if (e.button === 2) {
      // 右键：切换菜单（透明窗口上 contextmenu 事件不可靠，用 pointerdown 触发）
      // 贴边时先滑出，保证菜单完整可见、操作落在球上
      e.preventDefault();
      e.stopPropagation();
      revealIfDocked();
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
          offPhys: Math.round(((canvas - ballPx) / 2) * scale),
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
    // 窗口是画布、球居中偏 offPhys：钳制对象是球边而非窗口角
    return {
      x: Math.min(Math.max(x, d.monLeft - d.offPhys), Math.max(d.monLeft - d.offPhys, d.monRight - ballPhys - d.offPhys)),
      y: Math.min(Math.max(y, d.monTop - d.offPhys), Math.max(d.monTop - d.offPhys, d.monBottom - ballPhys - d.offPhys)),
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
      void emit("va:drag:start").catch(() => {});
      // 拖拽期间强制接受点击（防穿透态抢走指针事件）
      void setBallPassthroughOverride(true).catch(() => {});
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
      void emit("va:drag:end").catch(() => {});
      void setBallPassthroughOverride(false).catch(() => {});
      const target = dragTargetRef.current;
      dragTargetRef.current = null;
      if (target) {
        void getCurrentWindow()
          .setPosition(new PhysicalPosition(target.x, target.y))
          .catch(() => {});
        void saveFloatingBallPos(target.x, target.y).catch(() => {});
        maybeDock(target.x, d.scale, d.offPhys);
      } else {
        void getCurrentWindow()
          .outerPosition()
          .then((p) => {
            void saveFloatingBallPos(p.x, p.y).catch(() => {});
            maybeDock(p.x, d.scale, d.offPhys);
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
    void setBallPassthroughOverride(false).catch(() => {});
  }

  /** 悬停进入：已贴边则滑出 */
  function onBallPointerEnter() {
    revealIfDocked();
  }

  /** 悬停离开：处于可贴边状态则重新武装 1 秒倒计时（拖拽中有 pointer capture 不会误触发） */
  function onBallPointerLeave() {
    const p = dockParamsRef.current;
    if (p && !dockEdgeRef.current && !dragRef.current && !menuOpenRef.current) {
      armDock(p.edge, p.preDockX, p.monitorLeft, p.monitorRight, p.ballPhys, p.offPhys);
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
      {/* 音频波纹：播放中 + 标准模式 + 球可见（非球本体特效层） */}
      {playing && perfMode === "standard" && ballVisible && (
        <AudioRipples canvas={canvas} ballPx={ballPx} />
      )}

      {/* 球本体：角色 SVG 居中于画布 + 透明交互层（画布 = 球径×1.5，窗口尺寸永不变化） */}
      <div
        className={[
          "absolute transition-all duration-700 ease-in",
          despawning ? "scale-0 opacity-0" : "scale-100 opacity-100",
        ].join(" ")}
        style={{ left: off, top: off, width: ballPx, height: ballPx }}
      >
        <svg
          ref={svgRef}
          className="pointer-events-none absolute inset-0 h-full w-full overflow-visible transition-transform duration-500"
          // 右半屏水平镜像；启动演出期放大 1.45× 填满画布（progress 环更醒目），waking 后缩回
          style={{
            transform:
              `${mirrored ? "scaleX(-1) " : ""}${bootDone ? "" : "scale(1.45)"}`.trim() ||
              undefined,
          }}
          aria-hidden
        />
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
            "absolute inset-0 touch-none rounded-full bg-transparent",
            isDragging ? "cursor-grabbing" : "cursor-grab",
          ].join(" ")}
        />
        {/* 麦克风发送开启徽标：右上角绿色小球，尺寸/位置随球径缩放 */}
        {micOn && (
          <span
            aria-hidden
            className="absolute rounded-full bg-[#22c55e] shadow ring-1 ring-white/80"
            style={{
              width: Math.max(8, Math.round(ballPx * 0.16)),
              height: Math.max(8, Math.round(ballPx * 0.16)),
              right: -Math.max(1, Math.round(ballPx * 0.02)),
              top: -Math.max(1, Math.round(ballPx * 0.02)),
            }}
          />
        )}
      </div>

      {/* 右键菜单（展开窗口后在小球下方渲染） */}
      {menuOpen && (
        <div
          className="absolute w-52 rounded-2xl border border-[var(--ink-200)] bg-[var(--paper-card)] py-1.5 shadow-[0_16px_48px_rgba(26,24,22,0.35)]"
          style={{ top: off + ballPx + MENU_GAP, left: off }}
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
