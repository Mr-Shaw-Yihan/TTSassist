// 快捷键录入（紧凑行内版）：用于收藏夹给单个收藏绑定快捷键。
// 按下组合键识别 → 「绑定」确认 / 「取消」。

import { useState, useEffect } from "react";
import { buildAccelerator } from "../../utils/accelerator";

interface Props {
  onCapture: (hotkey: string) => void;
  onCancel: () => void;
}

export function HotkeyCapture({ onCapture, onCancel }: Props) {
  const [pending, setPending] = useState<string | null>(null);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      const accel = buildAccelerator(e);
      if (accel) setPending(accel);
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  return (
    <div className="space-y-1.5 rounded-xl border border-[var(--amber-500)] bg-[var(--amber-200)]/15 p-2">
      <div className="rounded-lg bg-[var(--paper-card)] px-3 py-1.5 text-center text-xs">
        {pending ? (
          <span className="font-mono font-medium text-[var(--amber-600)]">{pending}</span>
        ) : (
          <span className="animate-pulse text-[var(--ink-300)]">请按下快捷键…</span>
        )}
      </div>
      <div className="flex gap-1.5">
        <button
          onClick={() => pending && onCapture(pending)}
          disabled={!pending}
          className="flex-1 rounded-lg bg-[var(--ink-900)] px-2 py-1 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
        >
          绑定
        </button>
        <button
          onClick={onCancel}
          className="flex-1 rounded-lg border border-[var(--ink-200)] px-2 py-1 text-xs text-[var(--ink-700)] transition-colors hover:bg-[var(--ink-100)]"
        >
          取消
        </button>
      </div>
    </div>
  );
}