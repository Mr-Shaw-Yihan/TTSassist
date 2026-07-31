import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * 安全地监听 Tauri 事件，处理 React StrictMode 开发模式下的异步注册竞态。
 *
 * 问题：开发模式 StrictMode 会「挂载→卸载→再挂载」。第一次挂载的异步 listen
 * 还没完成就被「卸载」（此时 unlisten 还是 null，清理无效），第二次挂载又注册一个，
 * 结果两个监听器都生效 → 事件触发多次（如收藏快捷键播放出现重音）。
 *
 * 解法：用 cancelled 标志。若 effect 在异步完成前被清理，异步完成后立即注销自己。
 */
export function useTauriListen<T>(
  event: string,
  handler: (payload: T) => void,
  deps: React.DependencyList,
) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const u = await listen<T>(event, (e) => handler(e.payload));
      if (cancelled) {
        u(); // 已被清理 → 立即注销，避免泄漏
      } else {
        unlisten = u;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}