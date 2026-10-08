// 主窗口几何记忆：尺寸/位置/最大化写入 localStorage（WebView 持久存储），
// 启动时恢复、拖动/缩放防抖保存、关闭前兜底保存。仅 main 窗口挂载。
// 选择 localStorage 而非 settings.json：纯前端零后端命令，避免与窗口无关的设置写入。
//
// 健壮性护栏（2026-10-08）：主窗是 visible=false 创建、悬浮球 boot 动画放完才显示，
// 期间可能触发 onResized/关闭兜底存到接近 1×1 的占位小值；setSize 又不会按 minWidth
// 兜底，导致下次冷启动恢复成一个几乎看不见的小窗。故：恢复前校验尺寸下限、保存前
// 拦截不可见/退化尺寸——从读写两端杜绝坏值。

import { useEffect } from "react";
import {
  getCurrentWindow,
  availableMonitors,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";

const KEY = "va-window-state";
const SAVE_DEBOUNCE_MS = 800;
// 恢复/保存认可的物理像素边长下限。配置最小尺寸为逻辑 360×540，任何合理缩放下
// 物理边长都远大于此阈值；低于阈值即视为占位/退化值（窗口未布局或未显示时读到的小值）。
const MIN_SIDE_PHYSICAL = 200;

interface WindowState {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  maximized?: boolean;
}

function readState(): WindowState | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as WindowState) : null;
  } catch {
    return null;
  }
}

function writeState(patch: WindowState) {
  try {
    const next = { ...readState(), ...patch };
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* 存储失败静默：几何记忆是非关键体验 */
  }
}

function plausibleSize(w?: number, h?: number): boolean {
  return !!w && !!h && w >= MIN_SIDE_PHYSICAL && h >= MIN_SIDE_PHYSICAL;
}

export function useWindowState() {
  useEffect(() => {
    const win = getCurrentWindow();
    let cancelled = false;
    let timer: number | null = null;

    // 启动恢复：最大化优先，否则恢复上次的尺寸与位置
    (async () => {
      const saved = readState();
      if (!saved) return;
      try {
        if (await win.isMaximized()) return; // 已是最大化（如系统记住的状态）则不覆盖
        // 仅恢复合理尺寸；异常小值（历史坏存档）忽略，保持配置默认窗口大小
        if (plausibleSize(saved.width, saved.height)) {
          await win.setSize(new PhysicalSize(saved.width!, saved.height!));
        }
        if (saved.x !== undefined && saved.y !== undefined) {
          // 仅当保存的位置落在任一显示器范围内才应用（防止拔掉外接屏后窗口失踪）
          const monitors = await availableMonitors();
          const inside = monitors.some((m) => {
            const mx = m.position.x;
            const my = m.position.y;
            return (
              saved.x! >= mx - 100 &&
              saved.x! < mx + m.size.width &&
              saved.y! >= my - 40 &&
              saved.y! < my + m.size.height
            );
          });
          if (inside) await win.setPosition(new PhysicalPosition(saved.x, saved.y));
        }
        if (saved.maximized) await win.maximize();
      } catch {
        /* 恢复失败用默认窗口尺寸 */
      }
    })();

    // 变化防抖保存（拖动/缩放高频事件）
    const scheduleSave = () => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void (async () => {
          if (cancelled) return;
          try {
            if (await win.isMaximized()) {
              writeState({ maximized: true });
              return;
            }
            // 窗口不可见（boot 前 visible=false / 收托盘隐藏）时不记尺寸，
            // 避免把未布局的占位小值持久化
            if (!(await win.isVisible())) return;
            const size: PhysicalSize = await win.innerSize();
            if (!plausibleSize(size.width, size.height)) return;
            const pos: PhysicalPosition = await win.outerPosition();
            writeState({ x: pos.x, y: pos.y, width: size.width, height: size.height, maximized: false });
          } catch {
            /* ignore */
          }
        })();
      }, SAVE_DEBOUNCE_MS);
    };

    const unResized = win.onResized(scheduleSave);
    const unMoved = win.onMoved(scheduleSave);

    // 关闭前兜底保存（防抖可能还没落盘）
    const unClose = win.onCloseRequested(() => {
      if (timer !== null) window.clearTimeout(timer);
      void (async () => {
        try {
          if (await win.isMaximized()) {
            writeState({ maximized: true });
            return;
          }
          if (!(await win.isVisible())) return;
          const size = await win.innerSize();
          if (!plausibleSize(size.width, size.height)) return;
          const pos = await win.outerPosition();
          writeState({ x: pos.x, y: pos.y, width: size.width, height: size.height, maximized: false });
        } catch {
          /* ignore */
        }
      })();
    });

    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
      void unResized.then((f) => f());
      void unMoved.then((f) => f());
      void unClose.then((f) => f());
    };
  }, []);
}
