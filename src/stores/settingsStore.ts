// Zustand store：全局共享的前端状态。
// 本阶段先放 settings（含 MiMo API Key），后续阶段加 messages/favorites。

import { create } from "zustand";
import { updateSetting } from "../services/invoke";
import type { Settings } from "../types";

interface SettingsState {
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  /** 增量更新一个键；返回后端写入后的完整 settings */
  patch: (key: keyof Settings, value: string | number | boolean) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: null,
  setSettings: (s) => set({ settings: s }),
  patch: async (key, value) => {
    const updated = await updateSetting(key as string, value);
    set({ settings: updated });
  },
}));