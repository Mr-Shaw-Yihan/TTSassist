// 首次启动询问「我的数据」存放位置（A 建，B 挂载）。
//
// 自包含：props 只有 onDone，pending 由组件自己向后端查询；确认成功后自行隐藏并回调。
// 数据默认在系统盘，体积大的用户（音频/样本）可以在这里直接换到其他盘，避免 C 盘爆满。

import { useCallback, useEffect, useState } from "react";
import {
  confirmDefaultContentRoot,
  getStorageReport,
  pickDirectory,
  setContentRoot,
  storageChoicePending,
  type StorageMoveReport,
  type StorageReport,
} from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { toast } from "../common/Toast";

interface FirstRunFlowDeps {
  notify: (message: string, tone?: "ok" | "err") => void;
  done: () => void;
}

export interface UseDefaultRootDeps extends FirstRunFlowDeps {
  confirmDefault: () => Promise<void>;
}

/** 「使用默认位置」：记下选择 → 关闭弹窗；失败保持弹窗让用户重试 */
export async function useDefaultRootFlow(deps: UseDefaultRootDeps): Promise<void> {
  try {
    await deps.confirmDefault();
    deps.done();
  } catch (e) {
    deps.notify(`保存位置失败：${e}`, "err");
  }
}

export interface ChooseOtherRootDeps extends FirstRunFlowDeps {
  pickDir: () => Promise<string | null>;
  move: (target: string) => Promise<StorageMoveReport>;
}

/** 「选择其他位置」：选目录 → 迁移并记下选择 → 关闭弹窗；取消选目录则什么都不做 */
export async function chooseOtherRootFlow(deps: ChooseOtherRootDeps): Promise<void> {
  const target = await deps.pickDir();
  if (!target) return;
  try {
    await deps.move(target);
    deps.notify(`「我的数据」将保存到 ${target}`);
    deps.done();
  } catch (e) {
    deps.notify(`更改位置失败：${e}`, "err");
  }
}

/** 「我的数据」当前体积：只算内容数据，日志与临时内容不算在内 */
function contentDataBytes(report: StorageReport): number {
  const contentKinds = new Set(["records", "audio", "samples", "plugin_data"]);
  return report.buckets
    .filter((b) => contentKinds.has(b.kind))
    .reduce((sum, b) => sum + b.bytes, 0);
}

export interface StorageFirstRunDialogViewProps {
  report: StorageReport;
  busy: string | null;
  onUseDefault: () => void;
  onPickOther: () => void;
}

/** 纯展示层（测试环境无 DOM，交互由导出的编排函数覆盖） */
export function StorageFirstRunDialogView({
  report,
  busy,
  onUseDefault,
  onPickOther,
}: StorageFirstRunDialogViewProps) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ink-900)]/30 p-4"
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="选择「我的数据」存放位置"
        className="animate-rise w-full max-w-lg rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] p-5 shadow-[0_8px_32px_rgba(26,24,22,0.14)]"
      >
        <h2 className="font-display text-base font-semibold text-[var(--ink-900)]">
          选择「我的数据」存放位置
        </h2>
        <p className="mt-2 text-xs leading-relaxed text-[var(--ink-500)]">
          消息记录、合成音频、收藏夹、克隆样本会存在这里。默认位置在系统盘，若体积较大建议改到其他盘。
          以后可在 设置 → 存储 中更改，更改时会自动迁移。
        </p>

        <div className="mt-3 space-y-1 rounded-lg border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3 py-2.5">
          <div className="flex items-baseline justify-between gap-3 text-[11px]">
            <span className="shrink-0 text-[var(--ink-300)]">当前位置</span>
            <span className="min-w-0 break-all text-right text-[var(--ink-700)]">{report.content_root}</span>
          </div>
          <div className="flex items-baseline justify-between gap-3 text-[11px]">
            <span className="shrink-0 text-[var(--ink-300)]">现有体积</span>
            <span className="text-[var(--ink-700)]">{formatBytes(contentDataBytes(report))}</span>
          </div>
        </div>

        {report.drives.length > 0 && (
          <div className="mt-3">
            <h3 className="mb-1 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
              各盘剩余空间
            </h3>
            <ul className="space-y-1 text-[11px]">
              {report.drives.map((drive) => (
                <li key={drive.letter} className="flex items-baseline justify-between gap-3">
                  <span className="text-[var(--ink-700)]">{drive.letter}</span>
                  <span className="text-[var(--ink-300)]">
                    剩余 {formatBytes(drive.free_bytes)} / 共 {formatBytes(drive.total_bytes)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}

        {report.legacy_exe_data_bytes > 0 && (
          <p className="mt-3 rounded-lg border border-[var(--amber-500)]/40 bg-[var(--amber-500)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
            检测到安装目录里有 {formatBytes(report.legacy_exe_data_bytes)} 旧数据；选择与安装目录同一块盘可以避免跨盘复制。
          </p>
        )}

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onPickOther}
            disabled={!!busy}
            className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)] disabled:opacity-50"
          >
            选择其他位置
          </button>
          <button
            onClick={onUseDefault}
            disabled={!!busy}
            className="rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {busy ?? "使用默认位置"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function StorageFirstRunDialog({ onDone }: { onDone?: () => void }) {
  const [pending, setPending] = useState<boolean | null>(null);
  const [report, setReport] = useState<StorageReport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const need = await storageChoicePending();
        if (cancelled) return;
        if (!need) {
          setPending(false);
          return;
        }
        const next = await getStorageReport();
        if (cancelled) return;
        setReport(next);
        setPending(true);
      } catch (e) {
        // 读不到就不挡住主界面：下次启动再问，避免首启直接被一个弹窗卡死
        console.error("读取「我的数据」位置失败", e);
        if (!cancelled) setPending(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const finish = useCallback(() => {
    setPending(false);
    onDone?.();
  }, [onDone]);

  const notify = useCallback((message: string, tone: "ok" | "err" = "ok") => toast(message, tone), []);

  async function handleDefault() {
    setBusy("正在保存…");
    try {
      await useDefaultRootFlow({ confirmDefault: confirmDefaultContentRoot, notify, done: finish });
    } finally {
      setBusy(null);
    }
  }

  async function handlePickOther() {
    setBusy("正在准备…");
    try {
      await chooseOtherRootFlow({
        pickDir: () => pickDirectory("选择「我的数据」的存放位置"),
        move: setContentRoot,
        notify,
        done: finish,
      });
    } finally {
      setBusy(null);
    }
  }

  if (!pending) return null;

  if (!report) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ink-900)]/30 p-4">
        <p className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3 text-xs text-[var(--ink-500)]">
          正在读取「我的数据」当前位置…
        </p>
      </div>
    );
  }

  return (
    <StorageFirstRunDialogView
      report={report}
      busy={busy}
      onUseDefault={() => void handleDefault()}
      onPickOther={() => void handlePickOther()}
    />
  );
}
