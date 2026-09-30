// 主窗口几何记忆：尺寸/位置/最大化写入 localStorage（WebView 持久存储），
// 启动时恢复、拖动/缩放防抖保存、关闭前兜底保存。仅 main 窗口挂载。
// 选择 localStorage 而非 settings.json：纯前端零后端命令，避免与窗口无关的设置写入。

import { useEffect } from "react";
import {
  getCurrentWindow,
  availableMonitors,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";

const KEY = "va-window-state";
const SAVE_DEBOUNCE_MS = 800;

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
        if (saved.width && saved.height) {
          await win.setSize(new PhysicalSize(saved.width, saved.height));
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
            const size: PhysicalSize = await win.innerSize();
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
          const size = await win.innerSize();
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
