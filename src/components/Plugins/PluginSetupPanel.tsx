// 插件安装进度面板：纯订阅者（阶段 21 重构）。
//
// 任务的【启动】在用户动作处经 usePluginTaskStore 发起，本组件只读 store
// 渲染当前任务，不再"挂载即启动"——从根上消除 StrictMode 双挂载重复触发、
// 以及切换页面导致任务孤儿的问题。挂在哪个页面、何时打开都不影响任务。
//
// 定量进度（percent>=0）显示百分比进度条；不定量（<0）显示脉冲条。

import { useState } from "react";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { importResourcePackFlow, cleanFailedResources } from "../../services/invoke";
import { ResourcePackLinks } from "./ResourcePackLinks";

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
  const [importing, setImporting] = useState(false);
  const [cleaning, setCleaning] = useState(false);

  // 无任务或任务属于其他插件 → 不渲染（任务仍在后台跑，切回可见）
  if (!task || task.pluginId !== pluginId) return null;

  const kindText = task.kind === "voice" ? "音色安装" : "环境安装";
  // 资源下载失败（如 GenieData 拉不动）→ 提供离线资源包导入入口
  const isResourceError =
    task.status === "error" && (task.error ?? "").includes("资源下载失败");

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

  /** 导入离线资源包：选 zip → 解压到插件数据目录 → 刷新状态 */
  async function handleImport() {
    try {
      setImporting(true);
      const done = await importResourcePackFlow(pluginId);
      if (done) {
        clear();
        onClosed?.();
      }
    } catch (e) {
      window.alert(`导入资源包失败：${e}`);
    } finally {
      setImporting(false);
    }
  }

  /** 清除失败资源：删掉不完整的下载产物，之后可重试或导入离线包 */
  async function handleClean() {
    const ok = window.confirm(
      "将删除已下载失败的语音资源（不影响运行环境与已装音色），删除后可重试在线下载或导入离线资源包。确定清除？"
    );
    if (!ok) return;
    try {
      setCleaning(true);
      const msg = await cleanFailedResources(pluginId);
      window.alert(msg);
    } catch (e) {
      window.alert(`清除失败：${e}`);
    } finally {
      setCleaning(false);
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
      {isResourceError && (
        <div className="mt-2 rounded-md border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5">
          <span className="block text-[10px] leading-relaxed text-[var(--ink-300)]">
            可开启代理（魔法上网）后重试；或清除失败资源后，从<ResourcePackLinks />
            下载「Genie 语音资源包」导入
          </span>
          <div className="mt-1.5 flex items-center gap-2">
            <button
              onClick={handleClean}
              disabled={cleaning || importing}
              className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)] disabled:opacity-50"
            >
              {cleaning ? "清除中…" : "清除失败资源"}
            </button>
            <button
              onClick={handleImport}
              disabled={importing || cleaning}
              className="shrink-0 rounded-md border border-[var(--amber-500)] px-2 py-0.5 text-[11px] text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-500)]/10 disabled:opacity-50"
            >
              {importing ? "导入中…" : "导入离线资源包"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
