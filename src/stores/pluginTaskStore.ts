// 插件安装任务全局状态（阶段 21）。
//
// 关键设计（修复"挂载即启动 + StrictMode 双挂载"缺陷）：
// 任务的【启动】放在用户动作处（确认框确认后 / 管理面板点按钮时），
// 由本 store 发起 invoke 并登记任务；进度面板（PluginSetupPanel）降级为
// 纯订阅者，只读 store + 接收进度事件，挂在哪个页面、何时打开都不影响任务。
//
// 全局单任务槽：与宿主 INSTALL_BUSY 对应，同一时刻只有一个安装任务；
// 任务进行中前端禁用其他安装入口，面板据此渲染。

import { create } from "zustand";
import { runPluginSetup, installVoice } from "../services/invoke";
import type { PluginSetupProgress } from "../types";

export type PluginTaskStatus = "running" | "done" | "error";

/** 当前安装任务（环境安装或音色安装） */
export interface PluginTask {
  pluginId: string;
  /** env=引擎环境安装，voice=音色安装 */
  kind: "env" | "voice";
  /** 音色 id（kind=voice 时有值） */
  voiceId?: string;
  /** 启动时的显示名（用于提示文案，不用内部 id） */
  label: string;
  /** 0~100 定量进度；<0 表示不定量（以 message 为准） */
  percent: number;
  message: string;
  status: PluginTaskStatus;
  /** status=error 时的错误信息 */
  error?: string;
}

interface PluginTaskStore {
  /** 当前任务；null = 空闲 */
  task: PluginTask | null;
  /** 启动引擎环境安装任务；resolve 结果文案，reject 中文错误 */
  startEnv: (pluginId: string, label: string) => Promise<string>;
  /** 启动音色安装任务 */
  startVoice: (pluginId: string, voiceId: string, label: string) => Promise<string>;
  /** 重试当前失败任务；无任务或非 error 返回 null */
  retry: () => Promise<string | null>;
  /** 进度事件入口（App 里监听 plugin-setup-progress 后调用） */
  applyProgress: (p: PluginSetupProgress) => void;
  /** 清除已终态任务（done/error） */
  clear: () => void;
}

export const usePluginTaskStore = create<PluginTaskStore>((set, get) => {
  /** 真正发起 invoke 并管理终态（startEnv/startVoice/retry 共用） */
  async function launch(task: PluginTask, invoke: () => Promise<string>): Promise<string> {
    set({ task });
    try {
      const msg = await invoke();
      // 成功后用插件返回的最终文案覆盖，进度拉满
      set({ task: { ...task, percent: 100, message: msg, status: "done" } });
      return msg;
    } catch (e) {
      const err = String(e);
      set({ task: { ...task, status: "error", error: err } });
      throw err;
    }
  }

  return {
    task: null,

    startEnv: (pluginId, label) => {
      const cur = get().task;
      if (cur?.status === "running") {
        return Promise.reject("有安装任务正在进行，请等待完成后再试");
      }
      const task: PluginTask = {
        pluginId,
        kind: "env",
        label,
        percent: -1,
        message: "正在准备…",
        status: "running",
      };
      return launch(task, () => runPluginSetup(pluginId));
    },

    startVoice: (pluginId, voiceId, label) => {
      const cur = get().task;
      if (cur?.status === "running") {
        return Promise.reject("有安装任务正在进行，请等待完成后再试");
      }
      const task: PluginTask = {
        pluginId,
        kind: "voice",
        voiceId,
        label,
        percent: -1,
        message: "正在准备…",
        status: "running",
      };
      return launch(task, () =>
        installVoice(pluginId, voiceId),
      );
    },

    retry: async () => {
      const cur = get().task;
      if (!cur || cur.status !== "error") return null;
      if (cur.kind === "voice" && cur.voiceId) {
        return get().startVoice(cur.pluginId, cur.voiceId, cur.label);
      }
      return get().startEnv(cur.pluginId, cur.label);
    },

    applyProgress: (p) => {
      const cur = get().task;
      // 只接收当前任务的进度（防止串台）
      if (!cur || cur.status !== "running") return;
      if (p.plugin_id !== cur.pluginId) return;
      if (cur.kind === "voice" && p.kind === "voice" && p.voice_id !== cur.voiceId) return;
      set({ task: { ...cur, percent: p.percent, message: p.message } });
    },

    clear: () => set({ task: null }),
  };
});

/** 派生：是否有任务正在进行（用于禁用其他安装入口） */
export function isTaskRunning(s: { task: PluginTask | null }): boolean {
  return s.task?.status === "running";
}
