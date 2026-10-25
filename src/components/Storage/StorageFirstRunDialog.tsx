// 首次启动询问「我的数据」存放位置（A 建，B 挂载）。
//
// 自包含：props 只有 onDone，pending 由组件自己向后端查询；确认成功后自行隐藏并回调。
// 数据默认在系统盘，体积大的用户（音频/样本）可以在这里直接换到其他盘，避免 C 盘爆满。

import { useCallback, useEffect, useState } from "react";
import {
  confirmDefaultContentRoot,
  getStorageReport,
  pickDirectory,
  restartApp,
  setContentRoot,
  storageChoicePending,
  type StorageMoveReport,
  type StorageReport,
} from "../../services/invoke";
import { formatBytes } from "../../utils/format";
import { showInfo } from "../common/ConfirmDialog";
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
  /** 搬家成功后告知“要重启”：点掉或超时都继续 */
  noticeRestart: (message: string) => Promise<void>;
  restart: () => Promise<void>;
}

/** 「选择其他位置」：选目录 → 迁移 → 重启生效；取消选目录则什么都不做 */
export async function chooseOtherRootFlow(deps: ChooseOtherRootDeps): Promise<void> {
  const target = await deps.pickDir();
  if (!target) return;
  try {
    const report = await deps.move(target);
    if (report.need_restart) {
      // 不重启就仍读写旧位置：那期间新发的消息会留在旧目录、下次启动“看不见”。
      // 所以说明完直接重启，不收“稍后再说”这种选项（要用户点一下才重启）。
      await deps.noticeRestart(
        "「我的数据」已搬到 " + target + "。\n软件要重启一次才会开始读写新位置，现在为你重启；若没有自动打开，请手动再启动一次。",
      );
      await deps.restart();
      return;
    }
    deps.notify(`「我的数据」将保存到 ${target}`);
    deps.done();
  } catch (e) {
    deps.notify(`更改位置失败：${e}`, "err");
  }
}

export interface FirstRunGateDeps {
  /** 门禁：是否仍需询问（storage_choice_pending） */
  pending: () => Promise<boolean>;
  /** 展示用：读一次存储报告（体积/盘符），可为慢/失败而不影响弹窗出现 */
  report: () => Promise<StorageReport>;
  /** 亮出弹窗 */
  show: () => void;
  /** 明确不需要弹（已确认过位置） */
  hide: () => void;
  setReport: (r: StorageReport) => void;
  setError: (message: string) => void;
}

/**
 * 首启门禁编排（根因修复 10-23）。
 *
 * 旧实现把「弹窗是否出现」与 getStorageReport() 绑在同一个 try：启动早期 report 一旦
 * reject 就被静默 catch 吞掉，弹窗永远不出现且用户看不到任何错误（真机复现的 bug）。
 * 现在：**可见性只由门禁 pending() 决定**——
 * - 门禁说要弹 → 立刻 show()，再异步补 report；report 失败只降级显示错误，弹窗照在。
 * - 门禁读取本身失败 → 保守 show()（宁可多问一次，不可漏问让用户不知道数据存哪）并显示错误。
 * - 门禁说不用弹 → hide()。
 */
export async function firstRunGateFlow(deps: FirstRunGateDeps): Promise<void> {
  let need: boolean;
  try {
    need = await deps.pending();
  } catch (e) {
    deps.setError(`读取存储状态失败：${e}`);
    deps.show();
    return;
  }
  if (!need) {
    deps.hide();
    return;
  }
  deps.show();
  try {
    deps.setReport(await deps.report());
  } catch (e) {
    deps.setError(`读取存储占用失败：${e}`);
  }
}

/**
 * 启动瞬态重试（10-23 真机铁证）。
 *
 * 后端 setup 里 AppState 是**刻意晚 manage** 的（搬家期间禁止前端往内容根写数据，
 * 见 lib.rs 同段注释），而本弹窗的 IPC 是前端最早发出的一批，撞上
 * “state not managed for field `state` on command `storage_choice_pending`” 属正常瞬态。
 * 旧实现一次失败就静默不弹（用户看到的就是“首启弹窗从来不出来”）；
 * 现在先延迟重试一次，仍失败才交给调用方降级显示。
 */
export async function withStartupRetry<T>(
  fn: () => Promise<T>,
  delayMs: number,
  sleep: (ms: number) => Promise<void> = (ms) =>
    new Promise((resolve) => setTimeout(resolve, ms)),
): Promise<T> {
  try {
    return await fn();
  } catch {
    await sleep(delayMs);
    return fn();
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
            <span className="shrink-0 text-[var(--ink-300)]">数据文件夹</span>
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
  // pending 只表示「门禁判定要不要弹」；可见性不再依赖 report 是否读到
  const [pending, setPending] = useState(false);
  const [report, setReport] = useState<StorageReport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // 降级可见：门禁/体积读取失败时把错误显示在弹窗里，不再静默吞掉
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void firstRunGateFlow({
      pending: () => withStartupRetry(storageChoicePending, 800),
      report: () => withStartupRetry(getStorageReport, 800),
      show: () => { if (!cancelled) setPending(true); },
      hide: () => { if (!cancelled) setPending(false); },
      setReport: (r) => { if (!cancelled) setReport(r); },
      setError: (m) => { if (!cancelled) { setLoadError(m); console.error(m); } },
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const finish = useCallback(() => {
    setPending(false);
    onDone?.();
  }, [onDone]);

  const notify = useCallback((message: string, tone: "ok" | "err" = "ok") => toast(message, tone), []);

  /** 搬家后的重启告知：**必须用户点一下「立即重启」才走**（真机实测 5 秒超时快到人没看清），
   *  而且这框是模态的，不存在“看完继续发消息写到旧位置”的窗口期。 */
  const noticeRestart = useCallback(
    (message: string) =>
      showInfo({ title: "数据已搬家，需要重启一次", message, confirmText: "立即重启" }),
    [],
  );

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
        noticeRestart,
        restart: restartApp,
        done: finish,
      });
    } finally {
      setBusy(null);
    }
  }

  if (!pending) return null;

  // report 尚未读到（正在算 / 读取失败）：弹窗仍要出现，降级显示。
  // 两个按钮都不依赖 report（confirmDefaultContentRoot / pickDirectory+setContentRoot），
  // 所以即使体积信息缺失，用户仍能完成选择，不是死路。
  if (!report) {
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
          {loadError ? (
            <p className="mt-3 rounded-lg border border-[var(--seal)]/40 bg-[var(--seal)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--seal)]">
              {loadError}（体积与盘符信息暂缺，但仍可直接选择位置）
            </p>
          ) : (
            <p className="mt-3 rounded-lg border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3 py-2 text-[11px] text-[var(--ink-500)]">
              正在读取当前位置与占用…
            </p>
          )}
          <div className="mt-4 flex justify-end gap-2">
            <button
              onClick={() => void handlePickOther()}
              disabled={!!busy}
              className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)] disabled:opacity-50"
            >
              选择其他位置
            </button>
            <button
              onClick={() => void handleDefault()}
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

  return (
    <StorageFirstRunDialogView
      report={report}
      busy={busy}
      onUseDefault={() => void handleDefault()}
      onPickOther={() => void handlePickOther()}
    />
  );
}
