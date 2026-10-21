// 残留引擎数据（插件已卸载但模型/运行环境还留着）的展示与清理。
//
// 设计文档 §六.2 要求它在「插件库」看得见，§六.3 的各类占用又在「设置-存储」里也要出现，
// 所以这里只把**渲染与删除动作**抽成一份（两个页面共用），数据抓取各自负责：
// 存储页已经有完整 report，插件页则等用户展开才去扫——避免一次展开扫两遍盘。

import { useState } from "react";
import { deletePluginData, getStorageReport, type OrphanData } from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { confirm } from "../common/ConfirmDialog";
import { toast } from "../common/Toast";

/** 删除一份残留数据：内部完成二次确认与提示。返回 true 表示调用方应当重拉数据。 */
export async function deleteOrphanData(orphan: OrphanData): Promise<boolean> {
  const ok = await confirm({
    title: `删除「${orphan.id}」的引擎数据`,
    danger: true,
    confirmText: "删除",
    message:
      `将删除约 ${formatBytes(orphan.bytes)} 引擎数据（模型与运行环境）：\n${orphan.path}\n\n` +
      "这个插件已经不在安装列表里，删掉后重新安装该插件需要重新下载。\n" +
      "这里只动引擎数据，不会删除消息、收藏、音频与克隆样本。\n\n确定要删除吗？",
  });
  if (!ok) return false;
  try {
    const freed = await deletePluginData(orphan.id);
    toast(`已释放 ${formatBytes(freed)}`);
    return true;
  } catch (e) {
    toast(`删除失败：${e}`, "err");
    return false;
  }
}

/** 残留列表本体（不含标题）。orphans 为空时不渲染任何东西。 */
export function OrphanRows({
  orphans,
  onChanged,
}: {
  orphans: OrphanData[];
  /** 删除成功后通知父级重拉数据 */
  onChanged?: () => void;
}) {
  const [busyId, setBusyId] = useState<string | null>(null);
  if (orphans.length === 0) return null;

  return (
    <ul className="space-y-1.5">
      {orphans.map((o) => (
        <li key={o.id} className="flex items-center justify-between gap-3 text-xs">
          <span className="min-w-0 flex-1 truncate" title={o.path}>
            <span className="text-[var(--ink-700)]">{o.id}</span>
            <span className="ml-2 tabular-nums text-[var(--ink-300)]">{formatBytes(o.bytes)}</span>
          </span>
          <button
            onClick={async () => {
              setBusyId(o.id);
              const changed = await deleteOrphanData(o);
              setBusyId(null);
              if (changed) onChanged?.();
            }}
            disabled={busyId !== null}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--seal)] transition-colors hover:border-[var(--seal)] disabled:opacity-50"
          >
            {busyId === o.id ? "删除中…" : "删除引擎数据"}
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * 插件库用的自取数版本：默认收起，展开时才扫盘（递归求和可能几万文件）。
 * 没有残留时整块不出现，不给用户看空壳。
 */
export function OrphanDataDisclosure({ onChanged }: { onChanged?: () => void }) {
  const [open, setOpen] = useState(false);
  const [orphans, setOrphans] = useState<OrphanData[] | null>(null);
  const [loading, setLoading] = useState(false);

  async function load() {
    setLoading(true);
    try {
      const r = await getStorageReport();
      setOrphans(r.orphans);
    } catch (e) {
      setOrphans([]);
      toast(`读取引擎数据失败：${e}`, "err");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mt-5">
      <button
        onClick={() => {
          const next = !open;
          setOpen(next);
          if (next && orphans === null) void load();
        }}
        className="flex w-full items-center gap-2 text-left"
      >
        <span className="h-3.5 w-[3px] rounded-full bg-[var(--ink-300)]" aria-hidden />
        <h2 className="font-display text-[13px] font-semibold text-[var(--ink-900)]">
          已下载引擎数据（未随插件安装）
        </h2>
        <span className="ml-auto text-[10.5px] text-[var(--ink-300)]">
          {loading ? "扫描中…" : orphans && orphans.length > 0 ? `${orphans.length} 项` : ""}
        </span>
      </button>
      {open && (
        <div className="mt-2 rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--ink-300)]">
            这里只是各插件下载的模型与运行环境，和「我的数据」（消息、收藏、音频、克隆样本）是两回事，删这里不影响后者。
          </p>
          {loading && <p className="text-xs text-[var(--ink-300)]">正在统计各插件数据目录…</p>}
          {!loading && orphans && orphans.length === 0 && (
            <p className="text-xs text-[var(--ink-300)]">
              没有多余的引擎数据。卸载插件时选「保留数据」，模型与运行环境会出现在这里。
            </p>
          )}
          {!loading && orphans && orphans.length > 0 && (
            <>
              <p className="mb-2 text-[11px] leading-relaxed text-[var(--ink-300)]">
                这些是卸载插件时选择「保留数据」留下的，重新安装对应插件可以直接接着用；不需要就删掉腾空间。
              </p>
              <OrphanRows
                orphans={orphans}
                onChanged={() => {
                  void load();
                  onChanged?.();
                }}
              />
            </>
          )}
        </div>
      )}
    </section>
  );
}
