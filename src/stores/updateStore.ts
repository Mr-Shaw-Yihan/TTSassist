// 版本更新状态（启动时检查一次）。
//
// 红点规则：只要有新版本（无论是否忽略过），设置入口与"关于"分类显示红点；
// 用户打开"关于"查看后，本次会话红点消失（下次启动若仍未升级会再次出现）。
// 启动弹窗规则：有新版本且该版本未被"忽略此版本"时才弹。

import { create } from "zustand";
import {
  cancelAppUpdate,
  checkAppUpdate,
  downloadAppUpdate,
  installAppUpdate,
} from "../services/invoke";
import type { DownloadedInfo, UpdateInfo, UpdateProgress } from "../types";

/** 应用内升级阶段 */
export type UpdatePhase = "idle" | "downloading" | "downloaded" | "failed" | "installing";

interface UpdateState {
  /** 检查到的新版本（null = 无更新 / 未检查 / 检查失败） */
  latest: UpdateInfo | null;
  /** 是否已完成一次检查 */
  checked: boolean;
  /** 本次会话更新弹窗已被关闭（稍后/忽略） */
  dialogDismissed: boolean;
  /** 本次会话已查看“关于”（红点消失） */
  aboutSeen: boolean;
  /** 下载阶段 */
  phase: UpdatePhase;
  /** 0~1；-1 = 总大小未知 */
  percent: number;
  /** 当前下载通道 */
  channel: string;
  /** 面向用户的中文说明（进度或失败原因） */
  message: string;
  /** 已下载并校验通过的安装包 */
  downloaded: DownloadedInfo | null;
  check: () => Promise<void>;
  dismissDialog: () => void;
  resetDialog: () => void;
  markAboutSeen: () => void;
  /** 开始下载（Gitee 优先，失败自动换 GitHub） */
  startDownload: () => Promise<void>;
  /** 进度事件入口（仅下载中采信） */
  onProgress: (p: UpdateProgress) => void;
  cancelDownload: () => Promise<void>;
  /** 拉起安装器并退出本程序 */
  install: () => Promise<void>;
  reset: () => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  latest: null,
  checked: false,
  dialogDismissed: false,
  aboutSeen: false,
  phase: "idle",
  percent: 0,
  channel: "",
  message: "",
  downloaded: null,
  check: async () => {
    try {
      const info = await checkAppUpdate();
      set({ latest: info, checked: true });
    } catch {
      // 网络失败静默：不打扰用户
      set({ latest: null, checked: true });
    }
  },
  dismissDialog: () => set({ dialogDismissed: true }),
  resetDialog: () => set({ dialogDismissed: false }),
  markAboutSeen: () => set({ aboutSeen: true }),
  startDownload: async () => {
    if (get().phase === "downloading" || get().phase === "installing") return;
    set({ phase: "downloading", percent: 0, channel: "", message: "准备下载…", downloaded: null });
    try {
      const d = await downloadAppUpdate();
      set({ phase: "downloaded", percent: 1, downloaded: d, message: "下载完成，校验通过" });
    } catch (e) {
      const msg = String(e ?? "下载失败");
      if (msg.includes("已取消")) {
        set({ phase: "idle", percent: 0, message: "" });
      } else {
        set({ phase: "failed", message: msg });
      }
    }
  },
  onProgress: (p) => {
    if (get().phase !== "downloading") return;
    set({ percent: p.percent, channel: p.channel, message: p.message });
  },
  cancelDownload: async () => {
    try {
      await cancelAppUpdate();
    } catch {
      // 取消指令发不出去也无碍，本地先回到空闲
    }
    set({ phase: "idle", percent: 0, channel: "", message: "已取消下载" });
  },
  install: async () => {
    const d = get().downloaded;
    if (!d || get().phase === "installing") return;
    set({ phase: "installing", message: "正在启动安装程序…" });
    try {
      await installAppUpdate(d.path);
    } catch (e) {
      set({ phase: "failed", message: String(e ?? "无法启动安装程序") });
    }
  },
  reset: () =>
    set({ phase: "idle", percent: 0, channel: "", message: "", downloaded: null }),
}));

/** 派生：是否需要显示红点（有新版本且本次会话未查看关于） */
export function shouldShowUpdateDot(s: UpdateState): boolean {
  return !!s.latest && !s.aboutSeen;
}
