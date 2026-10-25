// 设置页「存储」分区：把「东西存在哪、各占多大、能不能搬家」摊开给用户看。
//
// 设计文档 §六.3。挂在 `<Section title="存储">` 里即可——Section 收起时不渲染 children，
// 所以体积扫描（递归求和，大目录可能数万文件）天然延迟到用户展开时才跑。
//
// 口径（D7）：界面分「配置」与「我的数据」两区；消息 / 收藏 / 音频 / 克隆样本是珍贵资产，
// 界面不提供任何批量删除入口（曾有的一个清理按钮经用户裁定移除，见看板 10-23）。
//
// 诚实原则：界面只放后端真能执行的按钮。Conflict（新旧两处都有数据）在 1.9.0 没有对应的
// 处置命令，因此这里只给说明与两个「打开位置」，不做点了没反应的「合并」按钮。

import { useCallback, useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  getStorageReport,
  migrateStorageNow,
  pickDirectory,
  restartApp,
  setContentRoot,
  type StorageMoveReport,
  type StorageReport,
} from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { confirm, showInfo } from "../common/ConfirmDialog";
import { toast } from "../common/Toast";
import { SubPanel } from "../common/SettingsSection";
import { OrphanRows } from "../Plugins/OrphanDataSection";

const BTN_GHOST =
  "rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]";
const BTN_PRIMARY =
  "rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-50";
const AMBER_NOTE =
  "rounded-lg border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]";

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
  /** 搬家成功后告知“要重启”：用户点掉或超时都继续往下走 */
  noticeRestart: (message: string) => Promise<void>;
  /** 真正重启软件 */
  restart: () => Promise<void>;
}

/** 「更改数据位置」编排：选目录 → 确认 → 迁移 → 重启生效（纯逻辑，便于单测） */
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
    if (report.need_restart) {
      // 内容根是启动时一次性解析的：不重启就仍在读写旧位置，搬家后新发的消息会被
      // 留在旧目录、下次启动“看不见”（足够像丢数据）。所以不给“以后再说”的选项，
      // 说明完就重启（用户点一下“立即重启”）。
      await deps.noticeRestart(
        "已把「我的数据」搬到 " + target + "\n（迁移 " + report.moved_files + " 个文件 / " + formatBytes(report.moved_bytes) + "）。"
          + "\n\n软件要重启一次才会开始读写新位置，现在为你重启；若没有自动打开，请手动再启动一次。",
      );
      await deps.restart();
      return;
    }
    deps.notify("已更新「我的数据」位置");
  } catch (e) {
    deps.notify(`更改位置失败：${e}`, "err");
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
  onReload: () => void;
  onMigrate: () => void;
  onOpenPath: (path: string) => void;
}

/** 纯展示层：不做 IO，便于按文案与结构验收 */
export function StoragePanelView({
  report,
  busy,
  onChangeLocation,
  onReload,
  onMigrate,
  onOpenPath,
}: StoragePanelViewProps) {
  return (
    <div className="space-y-4">
      <SubPanel title="配置" desc="设置、快捷键、API Key、音色与数据位置指针；与「我的数据」分开存放，卸载时默认保留">
        <div className="space-y-3">
          <PathRow label="配置文件夹" path={report.config_root} onOpen={onOpenPath} />
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
        desc="消息记录、合成音频、收藏夹、克隆样本与引擎数据；与配置分开存放"
        right={
          <button onClick={onChangeLocation} disabled={!!busy} className={BTN_PRIMARY}>
            更改数据位置
          </button>
        }
      >
        <div className="space-y-3">
          <PathRow label="数据文件夹" path={report.content_root} onOpen={onOpenPath} />
          {busy && <p className="text-[11px] text-[var(--amber-600)]">{busy}</p>}
          {report.content_fallback && (
            <p className={AMBER_NOTE}>
              保存位置已失效或不可写，数据暂存在上面这个位置。重新选择一个可写的文件夹即可恢复。
            </p>
          )}
        </div>
      </SubPanel>

      {/* 搬家前留的原件：用户不需要操作它，但必须看得见它是什么、占多少
          （软件自动只留最近一份，不然就是磁盘上 invisible 的垃圾） */}
      {report.backups.length > 0 && (
        <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
          <p className="text-xs font-medium text-[var(--ink-700)]">
            搬家前留的原件（共 {report.backups.length} 份 / {formatBytes(report.backup_bytes)}）
          </p>
          <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
            更改数据位置、或升级时自动搬家，软件会把搬家前的原样文件留一份，万一事后发现数据不对可以退回来。
            它不影响正常使用，只占磁盘空间；软件只保留最近一次的原件，更早的会在下次搬家后自动删除。
          </p>
          <div className="mt-2 space-y-2">
            {report.backups.map((b, i) => (
              <PathRow
                key={b.path}
                label={(i === 0 ? "最近一份 · " : "更早一份 · ") + formatBytes(b.bytes)}
                path={b.path}
                onOpen={onOpenPath}
              />
            ))}
          </div>
        </div>
      )}

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

  /** 搬家后的重启告知：**必须用户点一下「立即重启」才走**。
   *  真机实测加 5 秒超时的话，人还没看清窗口就没了；而这个框是模态的（遮住输入），
   *  所以也不存在“看完继续发消息、写到旧位置”的窗口期。 */
  const noticeRestart = useCallback(
    (message: string) =>
      showInfo({ title: "数据已搬家，需要重启一次", message, confirmText: "立即重启" }),
    [],
  );
  const refresh = useCallback(() => {
    void reload();
  }, [reload]);

  async function doMigrate() {
    if (!report) return;
    const ok = await confirm({
      title: "迁移旧位置数据",
      message:
        `将把旧位置的数据（约 ${formatBytes(report.legacy_bytes)}）复制到新位置：\n${report.content_root}\n\n` +
        "原件不删除，会移入应用目录下的备份文件夹保留。迁移期间请不要关闭软件。",
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
            "迁移期间请不要关闭软件。旧位置的原件会保留一份在备份目录，可用来退回；软件只留最近一次的备份。",
          confirmText: "开始迁移",
        }),
      move: setContentRoot,
      setBusy,
      notify,
      noticeRestart,
      restart: restartApp,
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
      onReload={refresh}
      onMigrate={() => void doMigrate()}
      onOpenPath={(path) => void revealItemInDir(path).catch(() => toast("打开位置失败", "err"))}
    />
  );
}
