// pluginTaskStore 单测（诊断报告 test_coverage 高扇入项：fan-in 6 无测试）。
//
// 该 store 是插件安装任务的全局状态机，核心不变量都在纯逻辑里，适合单测锁定：
//   1. 单任务槽（running 时拒新任务，与宿主 INSTALL_BUSY 对应）；
//   2. launch 终态迁移（成功 percent=100/done、失败 error+reject）；
//   3. retry 按 kind/voiceId 正确分流；
//   4. applyProgress 的「防串台」守卫（plugin_id / voice_id / 非 running 均须忽略）；
//   5. clear 与派生函数 isTaskRunning。
// mock 掉 ../services/invoke 的 runPluginSetup / installVoice（同 settingsStore.test.ts 口径）。

import { describe, it, expect, vi, beforeEach } from "vitest";
import { usePluginTaskStore, isTaskRunning } from "./pluginTaskStore";
import type { PluginSetupProgress } from "../types";

vi.mock("../services/invoke", () => ({
  runPluginSetup: vi.fn(async (_pluginId: string) => "环境已就绪"),
  installVoice: vi.fn(async (_pluginId: string, _voiceId: string) => "音色已安装"),
}));

import { runPluginSetup, installVoice } from "../services/invoke";
const mockedRunEnv = vi.mocked(runPluginSetup);
const mockedRunVoice = vi.mocked(installVoice);

/** 可控挂起的 deferred，用于制造「running 未完成」的中间态 */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  usePluginTaskStore.setState({ task: null });
});

describe("pluginTaskStore", () => {
  it("初始为空闲（task=null）", () => {
    expect(usePluginTaskStore.getState().task).toBeNull();
  });

  it("startEnv 成功：调 runPluginSetup，终态 percent=100/done、message 用返回文案", async () => {
    mockedRunEnv.mockResolvedValueOnce("环境已就绪");
    const msg = await usePluginTaskStore.getState().startEnv("genie-tts", "Genie 引擎");
    expect(mockedRunEnv).toHaveBeenCalledWith("genie-tts");
    expect(msg).toBe("环境已就绪");
    const t = usePluginTaskStore.getState().task!;
    expect(t).toMatchObject({ pluginId: "genie-tts", kind: "env", status: "done", percent: 100, message: "环境已就绪" });
  });

  it("startEnv 失败：status=error、error 记录，且 promise 以中文错误 reject", async () => {
    mockedRunEnv.mockRejectedValueOnce("下载失败");
    await expect(usePluginTaskStore.getState().startEnv("genie-tts", "Genie")).rejects.toBe("下载失败");
    const t = usePluginTaskStore.getState().task!;
    expect(t.status).toBe("error");
    expect(t.error).toBe("下载失败");
  });

  it("startVoice 成功：kind=voice，调 installVoice(pluginId, voiceId)", async () => {
    mockedRunVoice.mockResolvedValueOnce("音色已安装");
    const msg = await usePluginTaskStore.getState().startVoice("minimax-tts", "v-01", "御姐音");
    expect(mockedRunVoice).toHaveBeenCalledWith("minimax-tts", "v-01");
    expect(msg).toBe("音色已安装");
    expect(usePluginTaskStore.getState().task).toMatchObject({ kind: "voice", voiceId: "v-01", status: "done" });
  });

  it("单任务槽：running 未结束时拒绝新任务，且不触发第二次 invoke", async () => {
    const d = deferred<string>();
    mockedRunEnv.mockReturnValueOnce(d.promise);
    const first = usePluginTaskStore.getState().startEnv("genie-tts", "Genie");
    // 此时应为 running
    expect(usePluginTaskStore.getState().task?.status).toBe("running");
    await expect(usePluginTaskStore.getState().startVoice("genie-tts", "v", "x")).rejects.toBe("有安装任务正在进行，请等待完成后再试");
    expect(mockedRunEnv).toHaveBeenCalledTimes(1);
    expect(mockedRunVoice).not.toHaveBeenCalled();
    // 收尾第一个任务，避免悬挂
    d.resolve("ok");
    await first;
  });

  it("retry：无任务返回 null", async () => {
    expect(await usePluginTaskStore.getState().retry()).toBeNull();
  });

  it("retry：done 任务返回 null（仅 error 可重试）", async () => {
    mockedRunEnv.mockResolvedValueOnce("ok");
    await usePluginTaskStore.getState().startEnv("genie-tts", "Genie");
    expect(usePluginTaskStore.getState().task?.status).toBe("done");
    expect(await usePluginTaskStore.getState().retry()).toBeNull();
  });

  it("retry：env 错误 → 重新走 startEnv", async () => {
    mockedRunEnv.mockRejectedValueOnce("boom");
    await expect(usePluginTaskStore.getState().startEnv("genie-tts", "Genie")).rejects.toBe("boom");
    mockedRunEnv.mockResolvedValueOnce("这次成功");
    const r = await usePluginTaskStore.getState().retry();
    expect(r).toBe("这次成功");
    expect(usePluginTaskStore.getState().task?.status).toBe("done");
  });

  it("retry：voice 错误 → 携带 voiceId 重新走 startVoice", async () => {
    mockedRunVoice.mockRejectedValueOnce("boom");
    await expect(usePluginTaskStore.getState().startVoice("minimax-tts", "v-9", "音")).rejects.toBe("boom");
    mockedRunVoice.mockResolvedValueOnce("重试成功");
    const r = await usePluginTaskStore.getState().retry();
    expect(mockedRunVoice).toHaveBeenLastCalledWith("minimax-tts", "v-9");
    expect(r).toBe("重试成功");
  });

  it("applyProgress：命中当前 running 任务的 plugin_id 时更新 percent/message", () => {
    // 手动置入 running 任务，隔离 launch 逻辑单测进度守卫
    usePluginTaskStore.setState({
      task: { pluginId: "genie-tts", kind: "env", label: "Genie", percent: -1, message: "正在准备…", status: "running" },
    });
    const p: PluginSetupProgress = { plugin_id: "genie-tts", kind: "env", percent: 42, message: "下载中 42%" };
    usePluginTaskStore.getState().applyProgress(p);
    expect(usePluginTaskStore.getState().task).toMatchObject({ percent: 42, message: "下载中 42%" });
  });

  it("applyProgress：plugin_id 不匹配时忽略（防串台）", () => {
    usePluginTaskStore.setState({
      task: { pluginId: "genie-tts", kind: "env", label: "Genie", percent: 10, message: "m", status: "running" },
    });
    usePluginTaskStore.getState().applyProgress({ plugin_id: "other", kind: "env", percent: 99, message: "x" });
    expect(usePluginTaskStore.getState().task).toMatchObject({ percent: 10, message: "m" });
  });

  it("applyProgress：voice 任务下 voice_id 不匹配时忽略", () => {
    usePluginTaskStore.setState({
      task: { pluginId: "minimax-tts", kind: "voice", voiceId: "v-1", label: "音", percent: 20, message: "m", status: "running" },
    });
    usePluginTaskStore.getState().applyProgress({ plugin_id: "minimax-tts", kind: "voice", voice_id: "v-2", percent: 88, message: "x" });
    expect(usePluginTaskStore.getState().task).toMatchObject({ percent: 20, message: "m" });
    // 命中当前 voiceId 才更新
    usePluginTaskStore.getState().applyProgress({ plugin_id: "minimax-tts", kind: "voice", voice_id: "v-1", percent: 55, message: "y" });
    expect(usePluginTaskStore.getState().task).toMatchObject({ percent: 55, message: "y" });
  });

  it("applyProgress：非 running（done/error/null）时忽略", () => {
    usePluginTaskStore.setState({
      task: { pluginId: "genie-tts", kind: "env", label: "Genie", percent: 100, message: "完成", status: "done" },
    });
    usePluginTaskStore.getState().applyProgress({ plugin_id: "genie-tts", kind: "env", percent: 3, message: "z" });
    expect(usePluginTaskStore.getState().task).toMatchObject({ percent: 100, status: "done" });
  });

  it("clear：终态任务可清除为 null", () => {
    usePluginTaskStore.setState({
      task: { pluginId: "genie-tts", kind: "env", label: "Genie", percent: 100, message: "完成", status: "done" },
    });
    usePluginTaskStore.getState().clear();
    expect(usePluginTaskStore.getState().task).toBeNull();
  });

  it("isTaskRunning：仅 running 为 true", () => {
    expect(isTaskRunning({ task: null })).toBe(false);
    const run = { pluginId: "p", kind: "env" as const, label: "L", percent: 5, message: "m", status: "running" as const };
    const done = { ...run, status: "done" as const };
    expect(isTaskRunning({ task: run })).toBe(true);
    expect(isTaskRunning({ task: done })).toBe(false);
  });
});
