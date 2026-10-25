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
  const notifyCalls: string[] = [];
  it("源码里有「配置」「我的数据」双区与更改位置入口，不出现「缓存」，且不再提供清理按钮", () => {
    expect(panelSource).toContain("配置");
    expect(panelSource).toContain("我的数据");
    expect(panelSource).toContain("更改数据位置");
    expect(panelSource).not.toContain("缓存");
    // 用户裁定移除批量清理入口（10-23）：防止未来误加回
    expect(panelSource).not.toContain("清理临时文件");
    expect(panelSource).not.toContain("cleanupStorageTemp");
  });

  it("搬家后不给“以后再说”：告知完就重启；并大白话展示「搬家前留的原件」", () => {
    // 内容根只在启动时解析，拖着不重启期间写的消息会落在旧位置“看不见”
    expect(panelSource).toContain("await deps.restart();");
    expect(panelSource).toContain("noticeRestart");
    // 备份可见：用户不需要操作它，但得知道它是什么、占多少
    expect(panelSource).toContain("搬家前留的原件");
    expect(panelSource).toContain("report.backups");
    // 不新增批量删除入口（用户裁定只自动只留一份，不给按钮）
    expect(panelSource).not.toContain("删除备份");
    // 真机实测过“5 秒自动重启”快到人没看清→ 必须用模态框等用户点一下，不得用超时 race
    expect(panelSource).not.toContain("Promise.race");
  });

  it("临时文件三类按用户口径命名，不把「缓存」露到界面", () => {
    expect(displayBucketLabel("cache", "缓存")).toBe("临时文件");
    expect(displayBucketLabel("logs", "日志")).toBe("日志");
    expect(displayBucketLabel("records", "消息与收藏")).toBe("消息与收藏");
  });

  it("更改位置：选目录 → 迁移 → 告知并重启 → 重新统计", async () => {
    const pickDir = vi.fn(async () => "D:\\我的数据");
    const confirmMove = vi.fn(async () => true);
    const move = vi.fn(async () => MOVE_REPORT);
    const setBusy = vi.fn();
    const notify = vi.fn();
    const reload = vi.fn();
    const noticeRestart = vi.fn(async (message: string) => {
      void message;
    });
    const restart = vi.fn(async () => {});

    await changeContentRootFlow({
      pickDir, confirmMove, move, setBusy, notify, noticeRestart, restart, reload,
    });

    expect(pickDir).toHaveBeenCalledTimes(1);
    expect(confirmMove).toHaveBeenCalledWith("D:\\我的数据");
    expect(move).toHaveBeenCalledWith("D:\\我的数据");
    // 不再是一句“重启后生效”的 toast：给出可读说明后直接重启
    expect(noticeRestart).toHaveBeenCalledTimes(1);
    expect(noticeRestart.mock.calls[0][0]).toContain("D:\\我的数据");
    expect(noticeRestart.mock.calls[0][0]).toContain("重启");
    expect(restart).toHaveBeenCalledTimes(1);
    expect(notify).not.toHaveBeenCalled();
    expect(setBusy.mock.calls).toEqual([["正在迁移…"], [null]]);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("后端说不需重启时：只提示，不打扰用户重启", async () => {
    const restart = vi.fn(async () => {});
    const noticeRestart = vi.fn(async (message: string) => {
      void message;
    });
    await changeContentRootFlow({
      pickDir: async () => "D:\\x",
      confirmMove: async () => true,
      move: async () => ({ ...MOVE_REPORT, need_restart: false }),
      setBusy: vi.fn(),
      notify: (m: string) => notifyCalls.push(m),
      noticeRestart,
      restart,
      reload: vi.fn(),
    });
    expect(notifyCalls).toEqual(["已更新「我的数据」位置"]);
    expect(noticeRestart).not.toHaveBeenCalled();
    expect(restart).not.toHaveBeenCalled();
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
      noticeRestart: vi.fn(async () => {}),
      restart: vi.fn(async () => {}),
      reload,
    });

    expect(move).not.toHaveBeenCalled();
    expect(setBusy).not.toHaveBeenCalled();
    expect(reload).not.toHaveBeenCalled();
  });
});
