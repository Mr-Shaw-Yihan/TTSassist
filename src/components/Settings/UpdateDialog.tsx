// 启动时的版本更新弹窗：发现新版本 → 前往下载 / 稍后 / 忽略此版本。

import { openUrl } from "@tauri-apps/plugin-opener";
import type { UpdateInfo } from "../../types";

interface Props {
  info: UpdateInfo;
  /** 稍后再说（关闭弹窗，下次启动仍会提示） */
  onLater: () => void;
  /** 忽略此版本（写入设置，该版本不再弹窗，关于页保留红点） */
  onIgnore: () => void;
}

export function UpdateDialog({ info, onLater, onIgnore }: Props) {
  async function goDownload() {
    try {
      await openUrl(info.url);
    } catch {
      window.prompt("无法自动打开浏览器，请手动访问以下地址：", info.url);
    }
    onLater();
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 animate-fade"
      onClick={onLater}
    >
      <div
        className="w-[440px] max-w-[90vw] rounded-2xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-5 shadow-xl animate-rise"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-base font-medium text-[var(--ink-900)]">
          发现新版本 <span className="font-mono text-[var(--amber-600)]">v{info.version}</span>
        </h2>

        {info.notes.trim() && (
          <div className="scrollbar-thin mt-3 max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg border border-[var(--ink-200)] bg-[var(--paper)] px-3 py-2.5 text-xs leading-relaxed text-[var(--ink-500)]">
            {info.notes.trim()}
          </div>
        )}

        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onIgnore}
            className="rounded-lg px-3 py-1.5 text-xs text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-500)]"
          >
            忽略此版本
          </button>
          <button
            onClick={onLater}
            className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)]"
          >
            稍后提醒
          </button>
          <button
            onClick={goDownload}
            className="rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90"
          >
            前往下载
          </button>
        </div>
      </div>
    </div>
  );
}
