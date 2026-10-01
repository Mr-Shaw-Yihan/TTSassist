// 插件商店卡（竖版，适配阵列网格）：身份行 → 属性徽标行 → 描述 → 主操作。
// 已安装的插件在商店里显示「已安装 ✓」（可更新时显示更新按钮，版本明示去向）。

import { supportsClone, type StoreItem } from "../../hooks/usePluginInventory";

export function StoreCard({
  item,
  busy,
  onInstall,
}: {
  item: StoreItem;
  busy: string | null;
  onInstall: (item: StoreItem) => void;
}) {
  // 主操作决策：可更新 → 更新；未安装 → 安装；已安装最新 → 已安装 ✓
  const mode: "update" | "install" | "done" = item.installed
    ? item.hasUpdate
      ? "update"
      : "done"
    : "install";

  return (
    <div className="flex flex-col rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3 shadow-[0_1px_2px_rgba(26,24,22,0.03)]">
      {/* 身份行：名称 + 版本 + 商店态徽标 */}
      <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
        <span className="text-[13px] font-medium text-[var(--ink-900)]">{item.name}</span>
        <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
          {item.installed && item.hasUpdate ? `v${item.installedVersion} → v${item.version}` : `v${item.version}`}
        </span>
        {mode === "update" && (
          <span className="rounded-md bg-[var(--amber-600)]/10 px-1.5 py-0.5 text-[10px] font-medium text-[var(--amber-600)]">
            可更新
          </span>
        )}
        {mode === "install" && (
          <span className="rounded-md border border-[var(--ink-200)] px-1.5 py-0.5 text-[10px] text-[var(--ink-500)]">
            可获取
          </span>
        )}
        {mode === "done" && (
          <span className="rounded-md bg-emerald-600/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-700">
            已安装
          </span>
        )}
      </div>

      {/* 属性徽标行（名称下方，统一一排） */}
      <div className="mt-1.5 flex flex-wrap gap-1">
        <span className="rounded border border-[var(--ink-200)] px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
          {item.category === "local" ? "本地离线" : "云端"}
        </span>
        {supportsClone(item.id) && (
          <span className="rounded border border-[var(--ink-200)] px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
            支持克隆
          </span>
        )}
        {item.source === "bundled" && (
          <span className="rounded bg-[var(--ink-100)]/70 px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
            内置分发
          </span>
        )}
      </div>

      {/* 描述 */}
      <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-500)]" title={item.description}>
        {item.description || "　"}
      </p>

      {/* 资源需求摘要 */}
      {item.requirements && (
        <p
          className="mt-1.5 truncate rounded bg-[var(--ink-100)]/40 px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]"
          title={item.requirements}
        >
          {item.requirements}
        </p>
      )}

      {/* 操作行 */}
      <div className="mt-auto flex flex-wrap items-center gap-0.5 pt-2.5">
        {mode === "update" && (
          <button
            onClick={() => onInstall(item)}
            disabled={busy !== null}
            className="rounded-lg border border-[var(--amber-500)] bg-[var(--amber-500)] px-2.5 py-1 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            更新至 v{item.version}
          </button>
        )}
        {mode === "install" && (
          <button
            onClick={() => onInstall(item)}
            disabled={busy !== null}
            className="rounded-lg bg-[var(--amber-500)] px-3 py-1 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            安装
          </button>
        )}
        {mode === "done" && (
          <span className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-300)]">
            已安装 ✓
          </span>
        )}
        <span className="flex-1" />
        <span className="text-[9.5px] text-[var(--ink-300)]">
          {item.source === "bundled" ? "离线即装" : "在线安装"}
        </span>
      </div>
    </div>
  );
}
