// StoragePanel 验收：vitest 是 node 环境且无 testing-library（同 B4 的处理），
// 所以「渲染后断言」改为纯函数行为测试 + ?raw 源码快照，保证文案与按钮真实存在。

import { describe, expect, it, vi } from "vitest";
import panelSource from "./StoragePanel.tsx?raw";
import {
  changeContentRootFlow,
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

describe("StoragePanel 双区与位置迁移", () => {
  it("源码里有「配置」「我的数据」双区与更改位置入口，不出现「缓存」，且不再提供清理按钮", () => {
    expect(panelSource).toContain("配置");
    expect(panelSource).toContain("我的数据");
    expect(panelSource).toContain("更改数据位置");
    expect(panelSource).not.toContain("缓存");
    // 用户裁定移除批量清理入口（10-23）：防止未来误加回
    expect(panelSource).not.toContain("清理临时文件");
    expect(panelSource).not.toContain("cleanupStorageTemp");
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
});
