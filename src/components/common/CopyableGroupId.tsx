// QQ 群号一键复制：设置「关于」页与应用内升级失败兜底共用同一份实现与常量，
// 避免两处各写一份而漂号。原先关于页用 DOM id + style 直改做气泡，这里换成常规 state。

import { useCallback, useEffect, useRef, useState } from "react";

/** 官方 QQ 群号（换群时只改这一处） */
export const QQ_GROUP = "690907648";

interface Props {
  /** 附加样式（如字号/颜色由所在区块决定） */
  className?: string;
  /** 气泡消失时长（ms） */
  tipMs?: number;
}

export function CopyableGroupId({ className = "", tipMs = 1500 }: Props) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = useCallback(() => {
    void navigator.clipboard
      .writeText(QQ_GROUP)
      .catch(() => {})
      .finally(() => {
        setCopied(true);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), tipMs);
      });
  }, [tipMs]);

  return (
    <button
      type="button"
      onClick={copy}
      title="点击复制群号"
      className={`relative font-mono text-[var(--ink-600)] underline decoration-dashed underline-offset-2 transition-colors hover:text-[var(--amber-600)] ${className}`}
    >
      {QQ_GROUP}
      <span
        aria-hidden
        className={`pointer-events-none absolute -top-6 left-1/2 -translate-x-1/2 rounded bg-[var(--ink-700)] px-1.5 py-0.5 text-[10px] text-[var(--paper)] transition-opacity ${
          copied ? "opacity-100" : "opacity-0"
        }`}
      >
        已复制
      </span>
    </button>
  );
}
