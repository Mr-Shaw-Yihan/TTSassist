// Promise 风格对话框：confirm（确认/取消）与 info（信息展示，正文可选中 + 一键复制全文）。
// 替代 window.confirm/alert 的阻塞式原生弹窗，安墨风格：纸卡 + 墨字，危险操作用朱砂确认键。
// 注意：页面卸载时若有未决弹窗，Host 卸载兜底 resolve(false)，调用方不会悬挂。

import { useEffect, useState } from "react";
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

export interface InfoOptions {
  title: string;
  /** 正文，支持 \n 换行；渲染为可选中文本 + 底部「复制全文」按钮 */
  message: string;
  /** 确认按钮文案，默认「知道了」 */
  confirmText?: string;
}

/** choice 的一个动作按钮 */
export interface ChoiceOption {
  label: string;
  /** 选中后 resolve 出去的值（由调用方定义） */
  value: string;
  /** 危险动作用朱砂 */
  danger?: boolean;
}

export interface ChoiceOptions {
  title: string;
  message: string;
  /** 至少两个；点遮罩 / Esc 等于取消（resolve null），不给「默认一边」的按钮 */
  actions: ChoiceOption[];
}

/** 三种弹窗共用的结算值：confirm 用 boolean，choice 用动作 value，取消/关闭为 false 或 null */
type ResolveValue = boolean | string | null;

interface DialogState {
  kind: "confirm" | "info" | "choice";
  opts: ConfirmOptions | InfoOptions | ChoiceOptions | null;
  resolve: ((v: ResolveValue) => void) | null;
}

const useDialogStore = create<DialogState>(() => ({ kind: "confirm", opts: null, resolve: null }));

/** 弹出确认框并等待用户选择；Esc / 点遮罩 / 取消 = false */
export function confirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    // 已有未决弹窗时先按取消结算，避免 promise 悬挂
    const prev = useDialogStore.getState();
    if (prev.opts && prev.resolve) prev.resolve(false);
    useDialogStore.setState({ kind: "confirm", opts, resolve: (v) => resolve(v === true) });
  });
}

/** 在多个动作里选一个；Esc / 点遮罩 / 取消 = null（调用方必须处理 null 分支） */
export function choose(opts: ChoiceOptions): Promise<string | null> {
  return new Promise((resolve) => {
    const prev = useDialogStore.getState();
    if (prev.opts && prev.resolve) prev.resolve(null);
    useDialogStore.setState({
      kind: "choice",
      opts,
      resolve: (v) => resolve(typeof v === "string" ? v : null),
    });
  });
}

/** 弹出信息框（正文可选中、可一键复制全文），关闭时 resolve */
export function showInfo(opts: InfoOptions): Promise<void> {
  return new Promise<void>((resolve) => {
    const prev = useDialogStore.getState();
    if (prev.opts && prev.resolve) prev.resolve(false);
    useDialogStore.setState({
      kind: "info",
      opts: { ...opts, confirmText: opts.confirmText ?? "知道了" },
      resolve: () => resolve(),
    });
  });
}

function settle(v: ResolveValue) {
  const { resolve } = useDialogStore.getState();
  resolve?.(v);
  useDialogStore.setState({ opts: null, resolve: null });
}

/** 按钮底色。写成字面量常量而不是拼接类名，Tailwind 扫描源码时才能看到完整 class */
const BTN_BASE = "rounded-lg px-3 py-1.5 text-xs font-medium transition-opacity hover:opacity-90";
const BTN_SEAL = "bg-[var(--seal)] text-[var(--paper)]";
const BTN_AMBER = "bg-[var(--amber-500)] text-[var(--paper)]";
const BTN_INK = "bg-[var(--ink-900)] text-[var(--paper)]";
const BTN_CANCEL =
  "rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]";

/** 对话框渲染宿主：挂在页面根部即可（confirm / info / choice 共用一个弹窗壳） */
export function ConfirmDialogHost() {
  const kind = useDialogStore((s) => s.kind);
  const opts = useDialogStore((s) => s.opts);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    // 键盘 Esc = 取消/关闭
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") settle(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 宿主卸载时兜底结算，调用方 promise 不悬挂
  useEffect(() => () => settle(false), []);

  // 关闭时重置复制态
  useEffect(() => {
    if (!opts) setCopied(false);
  }, [opts]);

  if (!opts) return null;

  // opts 是三种弹窗的联合体，用 `in` 做结构化收窄，不用类型断言
  const actions = "actions" in opts ? opts.actions : [];
  const danger = "danger" in opts && opts.danger === true;
  const primaryText =
    ("confirmText" in opts ? opts.confirmText : undefined) ?? (kind === "info" ? "知道了" : "确定");

  async function copyAll() {
    try {
      await navigator.clipboard.writeText(opts?.message ?? "");
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* 剪贴板失败：正文仍可手动选中复制 */
    }
  }

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
        <p
          className={[
            "mt-2 whitespace-pre-line text-xs leading-relaxed text-[var(--ink-500)]",
            kind === "info" ? "select-text" : "",
          ].join(" ")}
        >
          {opts.message}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          {/* info 模式：一键复制全文（URL 等关键信息方便用户粘贴出去） */}
          {kind === "info" && (
            <button
              onClick={() => void copyAll()}
              className={[
                "rounded-lg border px-3 py-1.5 text-xs transition-colors",
                copied
                  ? "border-emerald-500/40 text-emerald-600"
                  : "border-[var(--ink-200)] text-[var(--ink-500)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]",
              ].join(" ")}
            >
              {copied ? "已复制 ✓" : "复制全文"}
            </button>
          )}
          {kind === "choice" ? (
            <>
              {/* 多选一：「取消」永远排第一且不当默认选项，不给用户闭眼看错的机会 */}
              <button onClick={() => settle(null)} className={BTN_CANCEL}>
                取消
              </button>
              {actions.map((a) => (
                <button
                  key={a.value}
                  onClick={() => settle(a.value)}
                  className={[BTN_BASE, a.danger ? BTN_SEAL : BTN_AMBER].join(" ")}
                >
                  {a.label}
                </button>
              ))}
            </>
          ) : (
            <>
              {kind === "confirm" && (
                <button onClick={() => settle(false)} className={BTN_CANCEL}>
                  取消
                </button>
              )}
              <button
                onClick={() => settle(true)}
                className={[BTN_BASE, kind === "confirm" ? (danger ? BTN_SEAL : BTN_AMBER) : BTN_INK].join(" ")}
              >
                {primaryText}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
