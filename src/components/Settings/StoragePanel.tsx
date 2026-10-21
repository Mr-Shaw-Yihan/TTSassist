// 设置页「存储」分区：把「东西存在哪、各占多大、能不能搬家」摊开给用户看。
//
// 设计文档 §六.3。挂在 `<Section title="存储">` 里即可——Section 收起时不渲染 children，
// 所以体积扫描（递归求和，大目录可能数万文件）天然延迟到用户展开时才跑。
//
// 口径（D7）：界面分「配置」与「我的数据」两区；可清理的只有下载残留、日志与
// 可重建的临时内容，消息 / 收藏 / 音频 / 克隆样本永远不在清理范围内。
//
// 诚实原则：界面只放后端真能执行的按钮。Conflict（新旧两处都有数据）在 1.9.0 没有对应的
// 处置命令，因此这里只给说明与两个「打开位置」，不做点了没反应的「合并」按钮。

import { useCallback, useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  cleanupStorageTemp,
  getStorageReport,
  migrateStorageNow,
  pickDirectory,
  setContentRoot,
  type StorageMoveReport,
  type StorageReport,
} from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { confirm } from "../common/ConfirmDialog";
import { toast } from "../common/Toast";
import { SubPanel } from "../common/SettingsSection";
import { OrphanRows } from "../Plugins/OrphanDataSection";

const BTN_GHOST =
  "rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]";
const BTN_PRIMARY =
  "rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-50";
const AMBER_NOTE =
  "rounded-lg border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]";

/** 清理二次确认的固定文案：必须让用户一眼看到珍贵资产不会被删 */
export const CLEANUP_CONFIRM_MESSAGE =
  "将清理下载残留、日志与可重建的临时文件。\n\n不会删除消息、收藏、音频与克隆样本。";

/** 后端的 cache 桶在界面上叫「临时文件」，避免用户误以为它和内容数据有关 */
export function displayBucketLabel(kind: string, label: string): string {
  return kind === "cache" ? "临时文件" : label;
}

interface FlowDeps {
  setBusy: (value: string | null) => void;
  notify: (message: string, tone?: "ok" | "err") => void;
  reload: () => void;
}

export interface ChangeContentRootDeps extends FlowDeps {
  pickDir: () => Promise<string | null>;
  confirmMove: (target: string) => Promise<boolean>;
  move: (target: string) => Promise<StorageMoveReport>;
}

/** 「更改数据位置」编排：选目录 → 确认 → 迁移 → 提示 → 重新统计（纯逻辑，便于单测） */
export async function changeContentRootFlow(deps: ChangeContentRootDeps): Promise<void> {
  let target: string | null;
  try {
    target = await deps.pickDir();
  } catch (e) {
    deps.notify(`打开文件夹选择器失败：${e}`, "err");
    return;
  }
  if (!target) return;
  if (!(await deps.confirmMove(target))) return;

  deps.setBusy("正在迁移…");
  try {
    const report = await deps.move(target);
    deps.notify(
      report.need_restart
        ? `已迁移 ${report.moved_files} 个文件（${formatBytes(report.moved_bytes)}），重启后生效`
        : "已更新「我的数据」位置",
    );
  } catch (e) {
    deps.notify(`更改位置失败：${e}`, "err");
  } finally {
    deps.setBusy(null);
    deps.reload();
  }
}

export interface CleanupTempDeps extends FlowDeps {
  confirmCleanup: () => Promise<boolean>;
  clean: () => Promise<number>;
}

/** 「清理临时文件」编排：确认 → 清理 → 提示释放体积 → 重新统计 */
export async function cleanupTempFlow(deps: CleanupTempDeps): Promise<void> {
  if (!(await deps.confirmCleanup())) return;

  deps.setBusy("正在清理…");
  try {
    const bytes = await deps.clean();
    deps.notify(bytes > 0 ? `已清理 ${formatBytes(bytes)} 临时文件` : "没有可清理的临时文件");
  } catch (e) {
    deps.notify(`清理失败：${e}`, "err");
  } finally {
    deps.setBusy(null);
    deps.reload();
  }
}

/** 路径行：完整路径可选中复制，右侧带「打开位置」 */
function PathRow({
  label,
  path,
  onOpen,
}: {
  label: string;
  path: string;
  onOpen?: (path: string) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="text-[10px] uppercase tracking-[0.2em] text-[var(--ink-300)]">{label}</div>
        <div className="mt-0.5 break-all select-text text-xs text-[var(--ink-700)]">{path}</div>
      </div>
      {onOpen && (
        <button onClick={() => onOpen(path)} className={`${BTN_GHOST} shrink-0`}>
          打开位置
        </button>
      )}
    </div>
  );
}

function DriveRow({ letter, free, total }: { letter: string; free: number; total: number }) {
  const freePercent = total > 0 ? Math.max(0, Math.min(100, (free / total) * 100)) : 0;
  return (
    <li className="space-y-1">
      <div className="flex items-baseline justify-between gap-3 text-[11px]">
        <span className="text-[var(--ink-700)]">{letter}</span>
        <span className="text-[var(--ink-300)]">
          剩余 {formatBytes(free)} / 共 {formatBytes(total)}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-[var(--ink-200)]/60">
        <div className="h-full rounded-full bg-[var(--amber-500)]" style={{ width: `${freePercent}%` }} />
      </div>
    </li>
  );
}

export interface StoragePanelViewProps {
  report: StorageReport;
  busy: string | null;
  onChangeLocation: () => void;
  onCleanup: () => void;
  onReload: () => void;
  onMigrate: () => void;
  onOpenPath: (path: string) => void;
}

/** 纯展示层：不做 IO，便于按文案与结构验收 */
export function StoragePanelView({
  report,
  busy,
  onChangeLocation,
  onCleanup,
  onReload,
  onMigrate,
  onOpenPath,
}: StoragePanelViewProps) {
  return (
    <div className="space-y-4">
      <SubPanel title="配置" desc="设置、快捷键、API Key、音色与数据位置指针；卸载时默认保留">
        <div className="space-y-3">
          <PathRow label="配置根" path={report.config_root} onOpen={onOpenPath} />
          <PathRow label="插件" path={report.plugins_root} onOpen={onOpenPath} />
          {report.plugins_fallback && (
            <p className={AMBER_NOTE}>
              安装目录写不进去，插件代码已改存到上面的「插件」位置。装到可写目录（如用户目录）后会自动改回安装目录。
            </p>
          )}
        </div>
      </SubPanel>

      <SubPanel
        title="我的数据"
        desc="消息记录、合成音频、收藏夹、克隆样本与引擎数据"
        right={
          <button onClick={onChangeLocation} disabled={!!busy} className={BTN_PRIMARY}>
            更改数据位置
          </button>
        }
      >
        <div className="space-y-3">
          <PathRow label="当前位置" path={report.content_root} onOpen={onOpenPath} />
          {busy && <p className="text-[11px] text-[var(--amber-600)]">{busy}</p>}
          {report.content_fallback && (
            <p className={AMBER_NOTE}>
              保存位置已失效或不可写，数据暂存在上面这个位置。重新选择一个可写的文件夹即可恢复。
            </p>
          )}
        </div>
      </SubPanel>

      {report.drives.length > 0 && (
        <div>
          <h3 className="mb-2 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
            各盘剩余空间
          </h3>
          <ul className="space-y-2">
            {report.drives.map((drive) => (
              <DriveRow
                key={drive.letter}
                letter={drive.letter}
                free={drive.free_bytes}
                total={drive.total_bytes}
              />
            ))}
          </ul>
        </div>
      )}

      {report.migration === "migratable" && (
        <div className="rounded-xl border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 p-3.5">
          <h3 className="font-display text-sm font-semibold text-[var(--ink-900)]">发现旧位置的「我的数据」</h3>
          <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
            旧位置有约 {formatBytes(report.legacy_bytes)} 的消息与音频还没有搬到新位置。
            搬一次之后，旧位置只留下可回退的备份。
          </p>
          <div className="mt-2.5">
            <button onClick={onMigrate} disabled={!!busy} className={BTN_PRIMARY}>
              迁移旧数据
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
          <div className="flex gap-2">
            <button onClick={onCleanup} disabled={!!busy} className={BTN_GHOST}>
              清理临时文件
            </button>
            <button onClick={onReload} disabled={!!busy} className={BTN_GHOST}>
              重新统计
            </button>
          </div>
        </div>
        {report.buckets.length === 0 ? (
          <p className="text-xs text-[var(--ink-300)]">目前还没有产生任何内容数据。</p>
        ) : (
          <ul className="space-y-1.5">
            {report.buckets.map((b) => (
              <li key={b.kind} className="flex items-baseline justify-between gap-3 text-xs">
                <span className="shrink-0 text-[var(--ink-700)]">{displayBucketLabel(b.kind, b.label)}</span>
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
        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
          「清理临时文件」只清理下载残留、日志与可重建的临时内容；不会删除消息、收藏、音频与克隆样本。
        </p>
      </div>

      {report.orphans.length > 0 && (
        <div>
          <h3 className="mb-2 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
            已下载引擎数据（未随插件安装）
          </h3>
          <OrphanRows orphans={report.orphans} onChanged={onReload} />
          <p className="mt-1.5 text-[11px] text-[var(--ink-300)]">
            这些是卸载插件时选择「保留数据」留下的，重新安装对应插件可以直接接着用。
          </p>
        </div>
      )}
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

  const notify = useCallback((message: string, tone: "ok" | "err" = "ok") => toast(message, tone), []);
  const refresh = useCallback(() => {
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
      refresh();
    }
  }

  function doChangeLocation() {
    void changeContentRootFlow({
      pickDir: () => pickDirectory("选择「我的数据」的存放位置"),
      confirmMove: (target) =>
        confirm({
          title: "更改「我的数据」位置",
          message:
            `消息、收藏、音频、样本与引擎数据将迁移到：\n${target}\n\n` +
            "迁移期间请不要关闭软件。旧位置的原件会保留在备份目录，可随时找回。",
          confirmText: "开始迁移",
        }),
      move: setContentRoot,
      setBusy,
      notify,
      reload: refresh,
    });
  }

  function doCleanup() {
    void cleanupTempFlow({
      confirmCleanup: () =>
        confirm({
          title: "清理临时文件",
          message: CLEANUP_CONFIRM_MESSAGE,
          confirmText: "开始清理",
          danger: true,
        }),
      clean: cleanupStorageTemp,
      setBusy,
      notify,
      reload: refresh,
    });
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
    <StoragePanelView
      report={report}
      busy={busy}
      onChangeLocation={doChangeLocation}
      onCleanup={doCleanup}
      onReload={refresh}
      onMigrate={() => void doMigrate()}
      onOpenPath={(path) => void revealItemInDir(path).catch(() => toast("打开位置失败", "err"))}
    />
  );
}
