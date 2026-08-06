// VB-CABLE 驱动安装向导对话框。
// 功能：自动从 GitHub 下载驱动 → 解压 → 以管理员权限启动安装程序。
// 下载失败时提供手动下载链接（GitHub + 百度网盘）。

import { useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { downloadVbCable, installVbCable, checkVbCable } from "../../services/invoke";

interface Props {
  onClose: () => void;
  /** 安装完成（检测到驱动已装）后的回调 */
  onInstalled?: () => void;
}

type Stage = "idle" | "downloading" | "downloaded" | "installing" | "done" | "error";

interface Progress {
  stage: string;
  downloaded: number;
  total: number;
  error?: string;
}

const GITHUB_URL = "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip";
const BAIDU_URL = "https://pan.baidu.com/s/5ZS8XXVIsAJY3ubQggnV1ow#list/path=%2F";
const OFFICIAL_URL = "https://vb-audio.com/Cable/";

export function VbCableInstallDialog({ onClose, onInstalled }: Props) {
  const [stage, setStage] = useState<Stage>("idle");
  const [progress, setProgress] = useState<Progress>({ stage: "", downloaded: 0, total: 0 });
  const [errorMsg, setErrorMsg] = useState("");
  const [zipPath, setZipPath] = useState("");

  // 监听下载进度事件
  useEffect(() => {
    const unlisten = listen<Progress>("vbcable:download-progress", (event) => {
      setProgress(event.payload);
      if (event.payload.stage === "done") {
        setStage("downloaded");
      } else if (event.payload.stage === "error") {
        setStage("error");
        setErrorMsg(event.payload.error || "下载失败");
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const startDownload = useCallback(async () => {
    setStage("downloading");
    setErrorMsg("");
    try {
      const path = await downloadVbCable();
      setZipPath(path);
      setStage("downloaded");
    } catch (e) {
      setStage("error");
      setErrorMsg(String(e));
    }
  }, []);

  const startInstall = useCallback(async () => {
    if (!zipPath) return;
    setStage("installing");
    try {
      await installVbCable(zipPath);
      // 等待用户完成安装向导，延迟检测
      setStage("done");
      // 3 秒后检测是否安装成功
      setTimeout(async () => {
        const installed = await checkVbCable().catch(() => false);
        if (installed) {
          onInstalled?.();
        }
      }, 3000);
    } catch (e) {
      setStage("error");
      setErrorMsg(String(e));
    }
  }, [zipPath, onInstalled]);

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  };

  const percent = progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : 0;

  return (
    <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/40" onClick={onClose}>
      <div
        className="w-[340px] rounded-2xl border border-[var(--ink-200)] bg-[var(--paper)] p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="mb-3 text-sm font-semibold text-[var(--ink-700)]">
          安装 VB-CABLE 虚拟声卡驱动
        </h3>

        {/* 空闲状态：说明 + 开始按钮 */}
        {stage === "idle" && (
          <div className="text-[11px] leading-relaxed text-[var(--ink-500)]">
            <p className="mb-3">
              虚拟麦克风功能需要 VB-CABLE 虚拟声卡驱动。
              点击下方按钮自动从 GitHub 下载并安装（约 2MB）。
            </p>
            <div className="mb-3 flex gap-2">
              <button
                onClick={startDownload}
                className="flex-1 rounded-lg bg-[var(--amber-500)] px-3 py-2 text-xs font-medium text-[var(--paper)] transition-colors hover:bg-[var(--amber-600)]"
              >
                ⬇ 自动下载安装
              </button>
              <button
                onClick={onClose}
                className="rounded-lg border border-[var(--ink-200)] px-3 py-2 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)]"
              >
                取消
              </button>
            </div>
            <div className="rounded-lg bg-[var(--ink-100)]/50 px-3 py-2 text-[10px] text-[var(--ink-400)]">
              <p className="font-medium text-[var(--ink-500)]">无法从 GitHub 下载？</p>
              <p className="mt-1">手动下载后安装，重启电脑即可：</p>
              <div className="mt-1.5 flex flex-col gap-1">
                <a href={GITHUB_URL} target="_blank" rel="noreferrer"
                   className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]">
                  📦 GitHub 下载链接
                </a>
                <a href={BAIDU_URL} target="_blank" rel="noreferrer"
                   className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]">
                  📦 百度网盘下载
                </a>
                <a href={OFFICIAL_URL} target="_blank" rel="noreferrer"
                   className="text-[var(--ink-400)] underline underline-offset-2 hover:text-[var(--ink-500)]">
                  🌐 VB-Audio 官网
                </a>
              </div>
            </div>
          </div>
        )}

        {/* 下载中：进度条 */}
        {stage === "downloading" && (
          <div className="text-[11px] text-[var(--ink-500)]">
            <div className="mb-2 flex items-center justify-between">
              <span>正在下载…</span>
              <span className="tabular-nums">
                {formatBytes(progress.downloaded)} / {progress.total > 0 ? formatBytes(progress.total) : "未知"}
                {progress.total > 0 && ` (${percent}%)`}
              </span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-[var(--ink-200)]">
              <div
                className="h-full rounded-full bg-[var(--amber-500)] transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            </div>
            <p className="mt-2 text-[10px] text-[var(--ink-400)]">
              下载速度取决于网络状况。如果长时间无进度，可取消后手动下载。
            </p>
            <button
              onClick={onClose}
              className="mt-3 w-full rounded-lg border border-[var(--ink-200)] py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)]"
            >
              取消
            </button>
          </div>
        )}

        {/* 下载完成：安装按钮 */}
        {stage === "downloaded" && (
          <div className="text-[11px] text-[var(--ink-500)]">
            <p className="mb-3 text-[var(--ink-700)]">✓ 下载完成！点击安装将启动安装程序。</p>
            <p className="mb-3 text-[10px] text-[var(--amber-600)]">
              ⚠ 需要管理员权限，请在弹出的 UAC 对话框中点击"是"。
            </p>
            <div className="flex gap-2">
              <button
                onClick={startInstall}
                className="flex-1 rounded-lg bg-[var(--amber-500)] px-3 py-2 text-xs font-medium text-[var(--paper)] transition-colors hover:bg-[var(--amber-600)]"
              >
                🚀 启动安装程序
              </button>
              <button
                onClick={onClose}
                className="rounded-lg border border-[var(--ink-200)] px-3 py-2 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)]"
              >
                稍后
              </button>
            </div>
          </div>
        )}

        {/* 安装中 */}
        {stage === "installing" && (
          <div className="text-[11px] text-[var(--ink-500)]">
            <p className="mb-2">正在启动安装程序…</p>
            <p className="text-[10px] text-[var(--amber-600)]">
              请在弹出的安装向导中完成安装，安装完成后需重启电脑。
            </p>
          </div>
        )}

        {/* 完成 */}
        {stage === "done" && (
          <div className="text-[11px] text-[var(--ink-500)]">
            <p className="mb-2 text-[var(--ink-700)]">✓ 安装程序已启动</p>
            <p className="mb-3">
              请按安装向导完成安装，然后<b>重启电脑</b>使驱动生效。
            </p>
            <button
              onClick={onClose}
              className="w-full rounded-lg bg-[var(--amber-500)] py-2 text-xs font-medium text-[var(--paper)] transition-colors hover:bg-[var(--amber-600)]"
            >
              知道了
            </button>
          </div>
        )}

        {/* 错误 */}
        {stage === "error" && (
          <div className="text-[11px] text-[var(--ink-500)]">
            <p className="mb-2 text-[var(--seal)]">✗ {errorMsg || "操作失败"}</p>
            <p className="mb-3 text-[10px]">
              如果无法从 GitHub 下载，请尝试手动下载：
            </p>
            <div className="mb-3 flex flex-col gap-1 rounded-lg bg-[var(--ink-100)]/50 px-3 py-2 text-[10px]">
              <a href={GITHUB_URL} target="_blank" rel="noreferrer"
                 className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]">
                📦 GitHub 下载链接
              </a>
              <a href={BAIDU_URL} target="_blank" rel="noreferrer"
                 className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]">
                📦 百度网盘下载
              </a>
              <a href={OFFICIAL_URL} target="_blank" rel="noreferrer"
                 className="text-[var(--ink-400)] underline underline-offset-2 hover:text-[var(--ink-500)]">
                🌐 VB-Audio 官网
              </a>
            </div>
            <div className="flex gap-2">
              <button
                onClick={startDownload}
                className="flex-1 rounded-lg border border-[var(--ink-200)] py-1.5 text-xs text-[var(--ink-500)] transition-colors hover:bg-[var(--ink-100)]"
              >
                重试下载
              </button>
              <button
                onClick={onClose}
                className="flex-1 rounded-lg bg-[var(--ink-200)] py-1.5 text-xs text-[var(--ink-600)] transition-colors hover:bg-[var(--ink-300)]"
              >
                关闭
              </button>
            </div>
          </div>
        )}

        <p className="mt-3 text-[9px] leading-relaxed text-[var(--ink-300)]">
          VB-CABLE 是捐赠软件（donationware），来源 vb-audio.com。
          如觉得好用，欢迎向原作者捐赠支持。
        </p>
      </div>
    </div>
  );
}
