// 设置页「存储」分区：把「东西存在哪、各占多大、哪些是残留、能不能搬家」摊开给用户看。
//
// 设计文档 §六.3。挂在 `<Section title="存储">` 里即可——Section 收起时不渲染 children，
// 所以体积扫描（递归求和，大目录可能数万文件）天然延迟到用户展开时才跑。
//
// 诚实原则：界面只放后端真能执行的按钮。Conflict（新旧两处都有数据）在 1.9.0 没有对应的
// 处置命令，因此这里只给说明与两个「打开位置」，不做点了没反应的「合并」按钮。

import { useCallback, useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  getStorageReport,
  migrateStorageNow,
  type StorageReport,
} from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { confirm } from "../common/ConfirmDialog";
import { toast } from "../common/Toast";
import { OrphanRows } from "../Plugins/OrphanDataSection";

const BTN_GHOST =
  "rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]";
const BTN_PRIMARY =
  "rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-50";

/** 路径行：完整路径可选中复制，右侧带「打开位置」 */
function PathRow({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="text-[10px] uppercase tracking-[0.2em] text-[var(--ink-300)]">{label}</div>
        <div className="mt-0.5 break-all select-text text-xs text-[var(--ink-700)]">{path}</div>
      </div>
      <button
        onClick={() => void revealItemInDir(path).catch(() => toast("打开位置失败", "err"))}
        className={`${BTN_GHOST} shrink-0`}
      >
        打开位置
      </button>
    </div>
  );
}

export function StoragePanel() {
  const [report, setReport] = useState<StorageReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setReport(await getStorageReport());
    } catch (e) {
      // 扫描失败只在本分区报错，不能把整个设置页带走
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function doMigrate() {
    if (!report) return;
    const ok = await confirm({
      title: "迁移旧位置数据",
      message:
        `将把旧位置的数据（约 ${formatBytes(report.legacy_bytes)}）复制到新位置：\n${report.content_root}\n\n` +
        "原件不删除，会移入配置根的备份目录保留。迁移期间请不要关闭软件。",
      confirmText: "开始迁移",
    });
    if (!ok) return;
    setBusy("正在迁移…");
    try {
      const msg = await migrateStorageNow();
      toast(msg);
    } catch (e) {
      toast(`迁移失败：${e}`, "err");
    } finally {
      setBusy(null);
      void reload();
    }
  }

  if (loading) {
    return <p className="text-xs text-[var(--ink-300)]">正在统计各目录占用…（首次可能慢一些）</p>;
  }
  if (error) {
    return (
      <div className="space-y-2">
        <p className="text-xs text-[var(--seal)]">读取存储信息失败：{error}</p>
        <button onClick={() => void reload()} className={BTN_GHOST}>
          重试
        </button>
      </div>
    );
  }
  if (!report) return null;

  return (
    <div className="space-y-4">
      <div className="space-y-3">
        <PathRow label="配置根（设置与账号信息）" path={report.config_root} />
        <PathRow label="内容根（消息、音频、样本等）" path={report.content_root} />
        {report.content_fallback && (
          <p className="rounded-lg border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
            安装目录写不进去（或被指定的位置已失效），内容数据已回退存放在上面这个位置。
            想让数据跟着程序走，需要把软件装到一个可写的目录。
          </p>
        )}
      </div>

      {report.migration === "migratable" && (
        <div className="rounded-xl border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 p-3.5">
          <h3 className="font-display text-sm font-semibold text-[var(--ink-900)]">发现旧位置的 data</h3>
          <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
            旧位置有约 {formatBytes(report.legacy_bytes)} 的消息与音频还没有搬到新位置。
            搬一次之后，旧位置只留下可回退的备份。
          </p>
          <div className="mt-2.5">
            <button onClick={() => void doMigrate()} disabled={!!busy} className={BTN_PRIMARY}>
              {busy ?? "迁移旧数据"}
            </button>
          </div>
        </div>
      )}

      {report.migration === "conflict" && (
        <div className="rounded-xl border border-[var(--seal)]/40 bg-[var(--seal)]/5 p-3.5">
          <h3 className="font-display text-sm font-semibold text-[var(--seal)]">新旧两处都有数据</h3>
          <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
            这种情况软件不替你选哪一边——静默合并会丢数据。请先分别打开两个位置核对内容，
            需要处理时联系我们远程协助。（自动合并工具在后续版本提供）
          </p>
        </div>
      )}

      <div>
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">各类占用</h3>
          <button onClick={() => void reload()} className={BTN_GHOST}>
            重新统计
          </button>
        </div>
        {report.buckets.length === 0 ? (
          <p className="text-xs text-[var(--ink-300)]">目前还没有产生任何内容数据。</p>
        ) : (
          <ul className="space-y-1.5">
            {report.buckets.map((b) => (
              <li key={b.kind} className="flex items-baseline justify-between gap-3 text-xs">
                <span className="shrink-0 text-[var(--ink-700)]">{b.label}</span>
                <span className="min-w-0 flex-1 truncate text-right text-[var(--ink-300)]" title={b.path}>
                  {b.path}
                </span>
                <span className="w-20 shrink-0 text-right tabular-nums text-[var(--ink-500)]">
                  {formatBytes(b.bytes)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {report.orphans.length > 0 && (
        <div>
          <h3 className="mb-2 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
            已下载引擎数据（未随插件安装）
          </h3>
          <OrphanRows orphans={report.orphans} onChanged={() => void reload()} />
          <p className="mt-1.5 text-[11px] text-[var(--ink-300)]">
            这些是卸载插件时选择「保留数据」留下的，重新安装对应插件可以直接接着用。
          </p>
        </div>
      )}
    </div>
  );
}
