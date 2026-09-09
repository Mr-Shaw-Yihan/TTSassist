// 应用内升级控件：发现新版本 → 下载（Gitee 优先、失败自动换 GitHub）→ 校验 → 用户点击后
// 安装并重启；两通道都失败时给出可复制群号与手动直链。启动弹窗与设置「关于」页共用本组件，
// 避免两处各写一套升级交互（原先一处是按钮、一处是链接，行为不一致）。

import { useCallback, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useTauriListen } from "../../hooks/useTauriListen";
import { subtitleStatus } from "../../services/invoke";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { useUpdateStore } from "../../stores/updateStore";
import { CopyableGroupId } from "./CopyableGroupId";
import { MarkdownLite } from "./MarkdownLite";
import type { UpdateInfo, UpdateProgress } from "../../types";

/** 字节 → 便于阅读的 MB */
function fmtMB(bytes: number): string {
  if (!bytes || bytes <= 0) return "约 20 MB";
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

interface Props {
  info: UpdateInfo;
  /** 是否渲染更新说明（弹窗里要，紧凑场合可关） */
  showNotes?: boolean;
  className?: string;
}

export function AppUpdater({ info, showNotes = true, className = "" }: Props) {
  const phase = useUpdateStore((s) => s.phase);
  const percent = useUpdateStore((s) => s.percent);
  const channel = useUpdateStore((s) => s.channel);
  const message = useUpdateStore((s) => s.message);
  const startDownload = useUpdateStore((s) => s.startDownload);
  const cancelDownload = useUpdateStore((s) => s.cancelDownload);
  const installNow = useUpdateStore((s) => s.install);
  const reset = useUpdateStore((s) => s.reset);
  const onProgress = useUpdateStore((s) => s.onProgress);

  // 点了「立即安装」但还在等后端回复时，先把按钮锁住
  const [arming, setArming] = useState(false);

  useTauriListen<UpdateProgress>("app-update-progress", onProgress, [onProgress]);

  const goReleasePage = useCallback(() => {
    void openUrl(info.url).catch(() => {});
  }, [info.url]);

  /** 安装会退出程序：字幕监听或插件安装进行中时先二次确认 */
  const handleInstall = useCallback(async () => {
    setArming(true);
    try {
      const [sub, busyTask] = await Promise.all([
        subtitleStatus().catch(() => null),
        Promise.resolve(usePluginTaskStore.getState().task?.status === "running"),
      ]);
      if (sub?.running || busyTask) {
        const ok = await confirm(
          "字幕监听或引擎安装正在进行，升级会退出程序并中断它们。仍要现在安装吗？",
          { title: "升级会中断当前任务", kind: "warning" },
        );
        if (!ok) return;
      }
      await installNow();
    } finally {
      setArming(false);
    }
  }, [installNow]);

  const barWidth = percent >= 0 ? Math.round(Math.min(1, Math.max(0, percent)) * 100) : 100;

  // ── 无下载直链（旧回退通道发现的新版本）──
  if (!info.has_download) {
    return (
      <div className={className}>
        {showNotes && info.notes.trim() && (
          <MarkdownLite text={info.notes} className="mt-1 max-h-56 overflow-y-auto" />
        )}
        <div className="mt-3 rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/50 px-3 py-2 text-[11px] leading-relaxed text-[var(--ink-500)]">
          本次更新请先前往下载页安装；升级到 v1.8.5 起即可在软件内一键升级。
        </div>
        <div className="mt-3 flex justify-end">
          <button
            onClick={goReleasePage}
            className="rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90"
          >
            前往下载页
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={className}>
      {showNotes && info.notes.trim() && (
        <MarkdownLite text={info.notes} className="max-h-56 overflow-y-auto" />
      )}

      {/* ── 下载中 ── */}
      {phase === "downloading" && (
        <div className="mt-3">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--ink-100)]">
            <div
              className={`h-full rounded-full bg-[var(--amber-500)] transition-[width] duration-300 ${
                percent < 0 ? "animate-pulse" : ""
              }`}
              style={{ width: percent < 0 ? "100%" : `${barWidth}%` }}
            />
          </div>
          <div className="mt-1.5 flex items-center justify-between text-[11px] text-[var(--ink-400)]">
            <span className="truncate">{message || "准备下载…"}</span>
            <span className="ml-2 shrink-0 font-mono">
              {percent >= 0 ? `${barWidth}%` : ""}
              {channel ? ` · ${channel}` : ""}
            </span>
          </div>
          <div className="mt-3 flex justify-end">
            <button
              onClick={() => void cancelDownload()}
              className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)]"
            >
              取消下载
            </button>
          </div>
        </div>
      )}

      {/* ── 待下载（idle）与失败（failed）共用入口区，失败时多给兜底信息 ── */}
      {(phase === "idle" || phase === "failed") && (
        <div className="mt-3">
          {phase === "failed" ? (
            <div className="rounded-lg border border-[var(--seal)]/40 bg-[var(--seal)]/10 px-3 py-2 text-[11px] leading-relaxed text-[var(--seal)]">
              下载失败：{message || "未知原因"}
              <div className="mt-1.5 text-[var(--ink-500)]">
                两条通道都不通时，可加群取同一版本的安装包（群文件）：
                <span className="ml-1 inline-flex items-center gap-1">
                  QQ 群 <CopyableGroupId />
                </span>
              </div>
              <div className="mt-1.5 flex flex-col gap-1 text-[var(--ink-500)]">
                <button
                  onClick={() => {
                    void openUrl(info.gitee_url).catch(() => {});
                  }}
                  className="w-fit text-[11px] text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]"
                >
                  Gitee 直链
                </button>
                <button
                  onClick={() => {
                    void openUrl(info.github_url).catch(() => {});
                  }}
                  className="w-fit text-[11px] text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]"
                >
                  GitHub 直链
                </button>
              </div>
            </div>
          ) : (
            <div className="text-[11px] leading-relaxed text-[var(--ink-400)]">
              安装包约 {fmtMB(info.size)}，下载后自动校验完整性；升级会退出并重启本软件。
            </div>
          )}

          <div className="mt-3 flex items-center justify-end gap-2">
            {phase === "failed" && (
              <button
                onClick={goReleasePage}
                className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)]"
              >
                前往下载页
              </button>
            )}
            <button
              onClick={() => {
                if (phase === "failed") reset();
                void startDownload();
              }}
              className="rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90"
            >
              {phase === "failed" ? "重试（自动换通道）" : "立即升级"}
            </button>
          </div>
        </div>
      )}

      {/* ── 已下载待安装 ── */}
      {phase === "downloaded" && (
        <div className="mt-3">
          <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
            已下载并通过完整性校验。安装会退出并重启本软件；过程中可能弹一次防火墙提权（UAC）窗口，属正常。
          </div>
          <div className="mt-3 flex items-center justify-end gap-2">
            <button
              onClick={() => reset()}
              disabled={arming}
              className="rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)] disabled:opacity-50"
            >
              稍后
            </button>
            <button
              onClick={() => void handleInstall()}
              disabled={arming}
              className="rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-xs font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-60"
            >
              {arming ? "准备中…" : "立即安装并重启"}
            </button>
          </div>
        </div>
      )}

      {/* ── 正在拉起安装器 ── */}
      {phase === "installing" && (
        <div className="mt-3 text-[11px] leading-relaxed text-[var(--ink-400)]">
          {message || "正在启动安装程序，本软件即将退出…"}
        </div>
      )}
    </div>
  );
}
