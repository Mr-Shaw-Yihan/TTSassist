// 插件安装进度面板：纯订阅者（阶段 21 重构）。
//
// 任务的【启动】在用户动作处经 usePluginTaskStore 发起，本组件只读 store
// 渲染当前任务，不再"挂载即启动"——从根上消除 StrictMode 双挂载重复触发、
// 以及切换页面导致任务孤儿的问题。挂在哪个页面、何时打开都不影响任务。
//
// 定量进度（percent>=0）显示百分比进度条；不定量（<0）显示脉冲条。

import { usePluginTaskStore } from "../../stores/pluginTaskStore";

export const EVENT_PLUGIN_SETUP_PROGRESS = "plugin-setup-progress";

interface Props {
  pluginId: string;
  /** 用户关闭面板（完成/失败后）的回调，父组件可借此刷新状态 */
  onClosed?: () => void;
}

export function PluginSetupPanel({ pluginId, onClosed }: Props) {
  const task = usePluginTaskStore((s) => s.task);
  const retry = usePluginTaskStore((s) => s.retry);
  const clear = usePluginTaskStore((s) => s.clear);

  // 无任务或任务属于其他插件 → 不渲染（任务仍在后台跑，切回可见）
  if (!task || task.pluginId !== pluginId) return null;

  const kindText = task.kind === "voice" ? "音色安装" : "环境安装";

  function handleClose() {
    clear();
    onClosed?.();
  }

  async function handleRetry() {
    try {
      await retry();
    } catch {
      /* 错误已记录在 store，面板会展示 */
    }
  }

  return (
    <div className="mt-2 rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2.5">
      {task.status === "error" ? (
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1 text-[11px] leading-relaxed text-[var(--seal)]">
            {kindText}失败：{task.error}
          </div>
          <button
            onClick={handleRetry}
            className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
          >
            重试
          </button>
          <button
            onClick={handleClose}
            className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
          >
            关闭
          </button>
        </div>
      ) : (
        <>
          <div className="mb-1.5 flex items-center gap-2">
            {task.percent >= 0 ? (
              <span className="font-mono text-[11px] text-sky-700">
                {Math.round(task.percent)}%
              </span>
            ) : (
              <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-sky-500" />
            )}
            <span className="min-w-0 flex-1 truncate text-[11px] text-sky-800" title={task.message}>
              {task.label}：{task.message}
            </span>
            {task.status === "done" && (
              <span className="shrink-0 text-[11px] text-emerald-600">✓ 完成</span>
            )}
          </div>
          {task.percent >= 0 ? (
            <div className="h-1.5 overflow-hidden rounded-full bg-sky-600/15">
              <div
                className="h-full rounded-full bg-sky-500 transition-[width] duration-300"
                style={{ width: `${Math.min(100, Math.max(0, task.percent))}%` }}
              />
            </div>
          ) : (
            <div className="h-1.5 overflow-hidden rounded-full bg-sky-600/15">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-sky-500/70" />
            </div>
          )}
          {task.status === "done" ? (
            <div className="mt-1 flex items-center justify-between">
              <span className="text-[10px] text-[var(--ink-300)]">{task.message}</span>
              <button
                onClick={handleClose}
                className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
              >
                关闭
              </button>
            </div>
          ) : (
            <div className="mt-1 text-[10px] text-[var(--ink-300)]">
              下载期间请勿关闭应用；中途失败可重试，已下载部分会自动续传跳过。
            </div>
          )}
        </>
      )}
    </div>
  );
}
