// 启动时的版本更新弹窗：发现新版本 → 软件内升级 / 稍后 / 忽略此版本。
// 升级交互（说明渲染、下载进度、安装、失败兜底）全在 <AppUpdater/> 里，与设置「关于」页共用；
// 下载状态存于 updateStore 全局态，所以关掉弹窗不会打断下载，再进来还能看到同一份进度。

import type { UpdateInfo } from "../../types";
import { AppUpdater } from "../common/AppUpdater";

interface Props {
  info: UpdateInfo;
  /** 稍后再说（关闭弹窗，下次启动仍会提示；不影响进行中的下载） */
  onLater: () => void;
  /** 忽略此版本（写入设置，该版本不再弹窗，关于页保留红点） */
  onIgnore: () => void;
}

export function UpdateDialog({ info, onLater, onIgnore }: Props) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 animate-fade"
      onClick={onLater}
    >
      <div
        className="w-[460px] max-w-[90vw] rounded-2xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-5 shadow-xl animate-rise"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-base font-medium text-[var(--ink-900)]">
          发现新版本 <span className="font-mono text-[var(--amber-600)]">v{info.version}</span>
        </h2>

        <AppUpdater info={info} className="mt-1" />

        <div className="mt-4 border-t border-[var(--ink-100)] pt-2.5">
          <button
            onClick={onIgnore}
            className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-500)]"
          >
            忽略此版本
          </button>
        </div>
      </div>
    </div>
  );
}
