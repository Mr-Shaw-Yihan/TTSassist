// updateStore 单测（T3 优先级 4）：应用内升级状态机（下载中→完成→失败回退），mock 掉 invoke。
// chime 的「音频上下文缺失时静默」分支也在这里一起覆盖（node 环境天然无 AudioContext）。

import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../services/invoke", () => ({
  checkAppUpdate: vi.fn(),
  cancelAppUpdate: vi.fn(async () => {}),
  downloadAppUpdate: vi.fn(),
  installAppUpdate: vi.fn(),
}));

import { useUpdateStore, shouldShowUpdateDot } from "./updateStore";
import { checkAppUpdate, downloadAppUpdate } from "../services/invoke";
import type { DownloadedInfo } from "../types";
import { playStartChime, playEndChime, playMicOnChime, playMicOffChime } from "../utils/chime";

const mockCheck = vi.mocked(checkAppUpdate);
const mockDownload = vi.mocked(downloadAppUpdate);

describe("updateStore 状态机", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUpdateStore.setState({
      latest: null,
      checked: false,
      dialogDismissed: false,
      aboutSeen: false,
      phase: "idle",
      percent: 0,
      channel: "",
      message: "",
      downloaded: null,
    });
  });

  it("检查成功：latest 写入 + checked 置位", async () => {
    mockCheck.mockResolvedValueOnce({ version: "1.9.0", notes: "" } as never);
    await useUpdateStore.getState().check();
    expect(useUpdateStore.getState().latest?.version).toBe("1.9.0");
    expect(useUpdateStore.getState().checked).toBe(true);
  });

  it("检查失败静默：latest 为 null 但 checked 置位", async () => {
    mockCheck.mockRejectedValueOnce(new Error("网络错误"));
    await useUpdateStore.getState().check();
    expect(useUpdateStore.getState().latest).toBeNull();
    expect(useUpdateStore.getState().checked).toBe(true);
  });

  it("下载中→完成：phase/percent/downloaded 推进", async () => {
    mockDownload.mockResolvedValueOnce({ path: "C:/tmp/setup.exe", size: 1024 } as never);
    await useUpdateStore.getState().startDownload();
    expect(useUpdateStore.getState().phase).toBe("downloaded");
    expect(useUpdateStore.getState().percent).toBe(1);
    expect(useUpdateStore.getState().downloaded?.path).toBe("C:/tmp/setup.exe");
  });

  it("下载失败→failed 回退；「已取消」→ idle 回退", async () => {
    mockDownload.mockRejectedValueOnce(new Error("校验不符"));
    await useUpdateStore.getState().startDownload();
    expect(useUpdateStore.getState().phase).toBe("failed");
    expect(useUpdateStore.getState().message).toContain("校验不符");

    useUpdateStore.getState().reset();
    mockDownload.mockRejectedValueOnce(new Error("已取消"));
    await useUpdateStore.getState().startDownload();
    expect(useUpdateStore.getState().phase).toBe("idle");
    expect(useUpdateStore.getState().message).toBe("");
  });

  it("下载中防重入：downloading 期间再次 startDownload 直接返回", async () => {
    let resolveDownload!: (v: DownloadedInfo) => void;
    mockDownload.mockReturnValueOnce(new Promise<DownloadedInfo>((r) => (resolveDownload = r)));
    const first = useUpdateStore.getState().startDownload();
    expect(useUpdateStore.getState().phase).toBe("downloading");
    await useUpdateStore.getState().startDownload(); // 第二次调用应被吞掉
    resolveDownload({ path: "x", version: "1.9.0", size: 1, sha256: "", channel: "" });
    await first;
    expect(mockDownload).toHaveBeenCalledTimes(1);
    expect(useUpdateStore.getState().phase).toBe("downloaded");
  });

  it("进度事件仅下载中采信：idle 时 onProgress 不改状态", () => {
    useUpdateStore.getState().onProgress({ percent: 0.5, channel: "gitee", message: "50%" } as never);
    expect(useUpdateStore.getState().percent).toBe(0);
    useUpdateStore.setState({ phase: "downloading" });
    useUpdateStore.getState().onProgress({ percent: 0.5, channel: "gitee", message: "50%" } as never);
    expect(useUpdateStore.getState().percent).toBe(0.5);
    expect(useUpdateStore.getState().channel).toBe("gitee");
  });

  it("红点派生：有新版本且未看关于 → 显示；看过 → 消失", () => {
    useUpdateStore.setState({ latest: { version: "1.9.0" } as never, aboutSeen: false });
    expect(shouldShowUpdateDot(useUpdateStore.getState())).toBe(true);
    useUpdateStore.getState().markAboutSeen();
    expect(shouldShowUpdateDot(useUpdateStore.getState())).toBe(false);
  });
});

describe("chime 音频不可用时静默（node 环境无 AudioContext）", () => {
  it("四个提示音函数不抛错", () => {
    expect(() => playStartChime()).not.toThrow();
    expect(() => playEndChime()).not.toThrow();
    expect(() => playMicOnChime()).not.toThrow();
    expect(() => playMicOffChime()).not.toThrow();
  });
});
