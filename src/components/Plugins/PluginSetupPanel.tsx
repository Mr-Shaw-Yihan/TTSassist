// 插件环境安装进度面板：挂载即开始执行 run_plugin_setup，
// 监听 plugin-setup-progress 事件展示进度，完成/失败后回调 onDone。
//
// 定量进度（percent>=0）显示百分比进度条；不定量（<0）显示脉冲文案。

import { useEffect, useState } from "react";
import { runPluginSetup } from "../../services/invoke";
import { useTauriListen } from "../../hooks/useTauriListen";
import type { PluginSetupProgress } from "../../types";

export const EVENT_PLUGIN_SETUP_PROGRESS = "plugin-setup-progress";

interface Props {
  pluginId: string;
  /** JSON 选项（如 {"voice":"mika"}），不传为完整环境安装 */
  options?: string;
  /** 结束回调：成功传结果文案，失败传 null */
  onDone: (message: string | null) => void;
}

export function PluginSetupPanel({ pluginId, options, onDone }: Props) {
  const [percent, setPercent] = useState(-1);
  const [message, setMessage] = useState("正在准备…");
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState(false);

  useTauriListen<PluginSetupProgress>(
    EVENT_PLUGIN_SETUP_PROGRESS,
    (p) => {
      if (p.plugin_id !== pluginId) return;
      setPercent(p.percent);
      setMessage(p.message);
    },
    [pluginId],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const msg = await runPluginSetup(pluginId, options);
        if (!cancelled) {
          setPercent(100);
          setMessage(msg);
          setFinished(true);
          // 稍作停留让用户看到"完成"，再通知父组件
          setTimeout(() => {
            if (!cancelled) onDone(msg);
          }, 1200);
        }
      } catch (e) {
        if (!cancelled) {
          setError(String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pluginId, options]);

  return (
    <div className="mt-2 rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2.5">
      {error ? (
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1 text-[11px] leading-relaxed text-[var(--seal)]">
            环境安装失败：{error}
          </div>
          <button
            onClick={() => onDone(null)}
            className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
          >
            关闭
          </button>
        </div>
      ) : (
        <>
          <div className="mb-1.5 flex items-center gap-2">
            {percent >= 0 ? (
              <span className="font-mono text-[11px] text-sky-700">
                {Math.round(percent)}%
              </span>
            ) : (
              <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-sky-500" />
            )}
            <span className="min-w-0 flex-1 truncate text-[11px] text-sky-800" title={message}>
              {message}
            </span>
            {finished && <span className="shrink-0 text-[11px] text-emerald-600">✓ 完成</span>}
          </div>
          {percent >= 0 ? (
            <div className="h-1.5 overflow-hidden rounded-full bg-sky-600/15">
              <div
                className="h-full rounded-full bg-sky-500 transition-[width] duration-300"
                style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
              />
            </div>
          ) : (
            <div className="h-1.5 overflow-hidden rounded-full bg-sky-600/15">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-sky-500/70" />
            </div>
          )}
          <div className="mt-1 text-[10px] text-[var(--ink-300)]">
            下载期间请勿关闭应用；中途失败可重新点击安装，已下载部分会自动续传跳过。
          </div>
        </>
      )}
    </div>
  );
}
