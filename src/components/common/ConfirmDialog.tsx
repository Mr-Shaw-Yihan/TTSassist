// Promise 风格确认弹窗：await confirm({...}) 返回是否确认。
// 替代 window.confirm 的阻塞式原生弹窗，安墨风格：纸卡 + 墨字，危险操作用朱砂确认键。
// 注意：页面卸载时若有未决弹窗，Host 卸载兜底 resolve(false)，调用方不会悬挂。

import { useEffect } from "react";
import { create } from "zustand";

export interface ConfirmOptions {
  title: string;
  /** 正文，支持 \n 换行 */
  message: string;
  /** 确认按钮文案，默认「确定」 */
  confirmText?: string;
  /** 危险操作：确认键用朱砂（卸载/删除类） */
  danger?: boolean;
}

interface ConfirmStore {
  opts: ConfirmOptions | null;
  resolve: ((v: boolean) => void) | null;
}

const useConfirmStore = create<ConfirmStore>(() => ({ opts: null, resolve: null }));

/** 弹出确认框并等待用户选择；Esc / 点遮罩 / 取消 = false */
export function confirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    // 已有未决弹窗时先按取消结算，避免 promise 悬挂
    const prev = useConfirmStore.getState();
    if (prev.opts && prev.resolve) prev.resolve(false);
    useConfirmStore.setState({ opts, resolve });
  });
}

function settle(v: boolean) {
  const { resolve } = useConfirmStore.getState();
  resolve?.(v);
  useConfirmStore.setState({ opts: null, resolve: null });
}

/** 确认弹窗渲染宿主：挂在页面根部即可 */
export function ConfirmDialogHost() {
  const opts = useConfirmStore((s) => s.opts);

  useEffect(() => {
    // 键盘 Esc = 取消
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") settle(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.addEventListener("keydown", onKey);
  }, []);

  // 宿主卸载时兜底结算，调用方 promise 不悬挂
  useEffect(() => () => settle(false), []);

  if (!opts) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ink-900)]/30 p-4"
      onClick={() => settle(false)}
      role="presentation"
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-label={opts.title}
        onClick={(e) => e.stopPropagation()}
        className="animate-rise w-full max-w-md rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-5 shadow-[0_8px_32px_rgba(26,24,22,0.14)]"
      >
        <h3 className="font-display text-sm font-semibold text-[var(--ink-900)]">{opts.title}</h3>
        <p className="mt-2 whitespace-pre-line text-xs leading-relaxed text-[var(--ink-500)]">
          {opts.message}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={() => settle(false)}
            className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            取消
          </button>
          <button
            onClick={() => settle(true)}
            className={[
              "rounded-lg px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90",
              opts.danger ? "bg-[var(--seal)]" : "bg-[var(--amber-500)]",
            ].join(" ")}
          >
            {opts.confirmText ?? "确定"}
          </button>
        </div>
      </div>
    </div>
  );
}
