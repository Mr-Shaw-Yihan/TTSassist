// 主窗口几何记忆：尺寸写 settings.json，位置/最大化写 localStorage。仅 main 窗口挂载。
//
// 2026-10-22 分工调整（B3 配合 A10）：尺寸不再由前端在启动后 setSize 恢复——主窗以
// visible=false 创建，Rust 在 setup 里读 settings.json 的 window_width/window_height
// 先 set_size 再 show，首帧即记忆尺寸。前端那一跳（配置默认尺寸露脸 → 横向弹开）正是
// 「先竖后横」的来源，故这里**只保存不恢复尺寸**。
// 位置与最大化还没迁进 settings（A10 范围外），仍走 localStorage 的读与写。
//
// 保留的健壮性护栏（2026-10-08）：窗口未布局/未显示时 innerSize 会读到接近 1×1 的占位小值，
// 一旦被记下，下次冷启动就是一个几乎看不见的小窗。故保存前一律过 plausibleSize 过滤。

import { useEffect } from "react";
import { getCurrentWindow, availableMonitors, PhysicalPosition } from "@tauri-apps/api/window";
import { updateSetting } from "../services/invoke";
import { windowSizeSettings } from "../utils/windowGeometry";

const KEY = "va-window-state";
const SAVE_DEBOUNCE_MS = 800;

interface WindowState {
  x?: number;
  y?: number;
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

    // 启动恢复：只恢复位置与最大化；尺寸由 Rust 在显示前应用
    (async () => {
      const saved = readState();
      if (!saved) return;
      try {
        if (await win.isMaximized()) return; // 已是最大化（如系统记住的状态）则不覆盖
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
        /* 恢复失败保持现状 */
      }
    })();

    /** 记一次当前几何：尺寸进 settings.json，位置/最大化进 localStorage */
    async function saveNow() {
      try {
        if (await win.isMaximized()) {
          writeState({ maximized: true });
          return;
        }
        // 窗口不可见（boot 前 visible=false / 收托盘隐藏）时不记尺寸，
        // 避免把未布局的占位小值持久化
        if (!(await win.isVisible())) return;
        const size = await win.innerSize();
        for (const [key, value] of windowSizeSettings(size.width, size.height)) {
          await updateSetting(key, value);
        }
        const pos = await win.outerPosition();
        writeState({ x: pos.x, y: pos.y, maximized: false });
      } catch {
        /* ignore */
      }
    }

    // 变化防抖保存（拖动/缩放高频事件）
    const scheduleSave = () => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (cancelled) return;
        void saveNow();
      }, SAVE_DEBOUNCE_MS);
    };

    const unResized = win.onResized(scheduleSave);
    const unMoved = win.onMoved(scheduleSave);

    // 关闭前兜底保存（防抖可能还没落盘）
    const unClose = win.onCloseRequested(() => {
      if (timer !== null) window.clearTimeout(timer);
      void saveNow();
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
