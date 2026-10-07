// 轻量 toast：模块级 mini store + 页面挂载 ToastHost 渲染。
// 替代 window.alert 的阻塞式弹窗，安墨风格：纸卡底 + 墨字 + 左侧琥珀/朱砂细条。
// 智能升级：err 类消息超过 60 字 / 含换行 / 含 URL 时，自动转为可复制的 showInfo
// 对话框（此类消息通常含用户需要阅读或带出去的内容，瞬态浮条来不及读）。

import { create } from "zustand";
import { showInfo } from "./ConfirmDialog";

export type ToastTone = "ok" | "err";

interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

interface ToastStore {
  items: ToastItem[];
  push: (message: string, tone: ToastTone) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;
const MAX_ITEMS = 3;
const AUTO_DISMISS_MS = 3500;

const useToastStore = create<ToastStore>((set) => ({
  items: [],
  push: (message, tone) =>
    set((s) => {
      const item = { id: nextId++, message, tone };
      // 超上限移除最旧，避免连续失败时堆满屏幕
      const items = [...s.items, item].slice(-MAX_ITEMS);
      window.setTimeout(() => {
        useToastStore.getState().dismiss(item.id);
      }, AUTO_DISMISS_MS);
      return { items };
    }),
  dismiss: (id) => set((s) => ({ items: s.items.filter((t) => t.id !== id) })),
}));

/** 命令式弹出提示（成功默认 ok，失败传 "err"）。
 *  err 类长消息（>60 字 / 含换行 / 含 URL）自动升级为可复制对话框 */
export function toast(message: string, tone: ToastTone = "ok"): void {
  const long =
    tone === "err" &&
    (message.length > 60 || message.includes("\n") || message.includes("http"));
  if (long) {
    void showInfo({ title: "操作失败", message });
    return;
  }
  useToastStore.getState().push(message, tone);
}

/** toast 渲染宿主：挂在页面根部，右下角浮现，自动消退 */
export function ToastHost() {
  const items = useToastStore((s) => s.items);
  const dismiss = useToastStore((s) => s.dismiss);
  if (items.length === 0) return null;
  return (
    <div className="pointer-events-none absolute bottom-12 right-4 z-20 flex flex-col items-end gap-2">
      {items.map((t) => (
        <button
          key={t.id}
          onClick={() => dismiss(t.id)}
          className={[
            "animate-rise pointer-events-auto flex max-w-sm items-start gap-2 rounded-xl border bg-[var(--paper-card)] px-3.5 py-2.5 text-left text-xs leading-relaxed shadow-[0_4px_16px_rgba(26,24,22,0.10)] transition-opacity",
            t.tone === "err"
              ? "border-[var(--seal)]/25 text-[var(--ink-700)]"
              : "border-[var(--ink-200)] text-[var(--ink-700)]",
          ].join(" ")}
        >
          <span
            aria-hidden
            className={[
              "mt-0.5 h-3.5 w-[3px] shrink-0 rounded-full",
              t.tone === "err" ? "bg-[var(--seal)]" : "bg-[var(--amber-500)]",
            ].join(" ")}
          />
          <span className="min-w-0 break-all">{t.message}</span>
        </button>
      ))}
    </div>
  );
}
