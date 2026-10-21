// StoragePanel 验收：vitest 是 node 环境且无 testing-library（同 B4 的处理），
// 所以「渲染后断言」改为纯函数行为测试 + ?raw 源码快照，保证文案与按钮真实存在。

import { describe, expect, it, vi } from "vitest";
import panelSource from "./StoragePanel.tsx?raw";
import {
  CLEANUP_CONFIRM_MESSAGE,
  changeContentRootFlow,
  cleanupTempFlow,
  displayBucketLabel,
} from "./StoragePanel";
import type { StorageMoveReport } from "../../services/invoke";

const MOVE_REPORT: StorageMoveReport = {
  moved_bytes: 2048,
  moved_files: 2,
  skipped: [],
  backup_dir: "C:\\backup",
  need_restart: true,
};

describe("StoragePanel 双区与安全清理", () => {
  it("源码里有「配置」「我的数据」双区与两个操作入口且不出现「缓存」", () => {
    expect(panelSource).toContain("配置");
    expect(panelSource).toContain("我的数据");
    expect(panelSource).toContain("更改数据位置");
    expect(panelSource).toContain("清理临时文件");
    expect(panelSource).not.toContain("缓存");
  });

  it("清理确认文案明确排除消息、收藏、音频与克隆样本", () => {
    expect(CLEANUP_CONFIRM_MESSAGE).toContain("不会删除消息、收藏、音频与克隆样本");
  });

  it("临时文件三类按用户口径命名，不把「缓存」露到界面", () => {
    expect(displayBucketLabel("cache", "缓存")).toBe("临时文件");
    expect(displayBucketLabel("logs", "日志")).toBe("日志");
    expect(displayBucketLabel("records", "消息与收藏")).toBe("消息与收藏");
  });

  it("更改位置：选目录 → 迁移 → 提示重启并重新统计", async () => {
    const pickDir = vi.fn(async () => "D:\\我的数据");
    const confirmMove = vi.fn(async () => true);
    const move = vi.fn(async () => MOVE_REPORT);
    const setBusy = vi.fn();
    const notify = vi.fn();
    const reload = vi.fn();

    await changeContentRootFlow({ pickDir, confirmMove, move, setBusy, notify, reload });

    expect(pickDir).toHaveBeenCalledTimes(1);
    expect(confirmMove).toHaveBeenCalledWith("D:\\我的数据");
    expect(move).toHaveBeenCalledWith("D:\\我的数据");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("重启后生效"));
    expect(setBusy.mock.calls).toEqual([["正在迁移…"], [null]]);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("更改位置：用户取消选目录就什么都不做", async () => {
    const move = vi.fn(async () => MOVE_REPORT);
    const setBusy = vi.fn();
    const reload = vi.fn();

    await changeContentRootFlow({
      pickDir: async () => null,
      confirmMove: async () => true,
      move,
      setBusy,
      notify: vi.fn(),
      reload,
    });

    expect(move).not.toHaveBeenCalled();
    expect(setBusy).not.toHaveBeenCalled();
    expect(reload).not.toHaveBeenCalled();
  });

  it("清理临时文件：确认后才执行，完成后提示释放体积并重新统计", async () => {
    const confirmCleanup = vi.fn(async () => true);
    const clean = vi.fn(async () => 2048);
    const setBusy = vi.fn();
    const notify = vi.fn();
    const reload = vi.fn();

    await cleanupTempFlow({ confirmCleanup, clean, setBusy, notify, reload });

    expect(clean).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith("已清理 2 KB 临时文件");
    expect(setBusy.mock.calls).toEqual([["正在清理…"], [null]]);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("清理临时文件：取消确认不删除任何东西", async () => {
    const clean = vi.fn(async () => 0);
    const reload = vi.fn();

    await cleanupTempFlow({
      confirmCleanup: async () => false,
      clean,
      setBusy: vi.fn(),
      notify: vi.fn(),
      reload,
    });

    expect(clean).not.toHaveBeenCalled();
    expect(reload).not.toHaveBeenCalled();
  });
});
