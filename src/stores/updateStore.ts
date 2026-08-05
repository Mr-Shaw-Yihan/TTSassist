// 版本更新状态（启动时检查一次）。
//
// 红点规则：只要有新版本（无论是否忽略过），设置入口与"关于"分类显示红点；
// 用户打开"关于"查看后，本次会话红点消失（下次启动若仍未升级会再次出现）。
// 启动弹窗规则：有新版本且该版本未被"忽略此版本"时才弹。

import { create } from "zustand";
import { checkAppUpdate } from "../services/invoke";
import type { UpdateInfo } from "../types";

interface UpdateState {
  /** 检查到的新版本（null = 无更新 / 未检查 / 检查失败） */
  latest: UpdateInfo | null;
  /** 是否已完成一次检查 */
  checked: boolean;
  /** 本次会话更新弹窗已被关闭（稍后/忽略） */
  dialogDismissed: boolean;
  /** 本次会话已查看"关于"（红点消失） */
  aboutSeen: boolean;
  check: () => Promise<void>;
  dismissDialog: () => void;
  markAboutSeen: () => void;
}

export const useUpdateStore = create<UpdateState>((set) => ({
  latest: null,
  checked: false,
  dialogDismissed: false,
  aboutSeen: false,
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
  markAboutSeen: () => set({ aboutSeen: true }),
}));

/** 派生：是否需要显示红点（有新版本且本次会话未查看关于） */
export function shouldShowUpdateDot(s: UpdateState): boolean {
  return !!s.latest && !s.aboutSeen;
}
