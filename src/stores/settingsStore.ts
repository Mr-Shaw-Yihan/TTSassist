// Zustand store：全局共享的前端状态。
// 本阶段先放 settings（含各引擎 API Key），后续阶段加 messages/favorites。

import { create } from "zustand";
import { updateSetting } from "../services/invoke";
import { persistTheme, resolveInitialTheme } from "../utils/theme";
import type { Settings, CustomVoiceItem } from "../types";

interface SettingsState {
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  /** 增量更新一个键；返回后端写入后的完整 settings */
  patch: (
    key: keyof Settings,
    value:
      | string
      | number
      | boolean
      | string[]
      | Record<string, string>
      | Record<string, Record<string, string>>
      | Record<string, CustomVoiceItem[]>,
  ) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: null,
  // 皮肤镜像一份到 localStorage：index.html 的内联同步脚本下次启动时首帧读取它
  // （真实 settings 要异步加载，赶不上首帧，只靠它会闪一下日间皮肤）
  setSettings: (s) => {
    persistTheme(resolveInitialTheme(s.theme));
    set({ settings: s });
  },
  patch: async (key, value) => {
    const updated = await updateSetting(key as string, value);
    if (key === "theme") persistTheme(resolveInitialTheme(String(value)));
    set({ settings: updated });
  },
}));