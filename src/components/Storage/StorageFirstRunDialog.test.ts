// 首启位置选择弹窗验收：vitest 为 node 环境且无 testing-library，
// 渲染分支用 ?raw 源码快照钉住，交互走导出的纯编排函数。

import { describe, expect, it, vi } from "vitest";
import dialogSource from "./StorageFirstRunDialog.tsx?raw";
import { chooseOtherRootFlow, useDefaultRootFlow } from "./StorageFirstRunDialog";
import type { StorageMoveReport } from "../../services/invoke";

const MOVE_REPORT: StorageMoveReport = {
  moved_bytes: 0,
  moved_files: 0,
  skipped: [],
  backup_dir: "",
  need_restart: false,
};

describe("StorageFirstRunDialog", () => {
  it("源码包含标题、默认/自选两个选项与位置说明", () => {
    expect(dialogSource).toContain("选择「我的数据」存放位置");
    expect(dialogSource).toContain("使用默认位置");
    expect(dialogSource).toContain("选择其他位置");
    expect(dialogSource).toContain("消息记录、合成音频、收藏夹、克隆样本会存在这里");
    expect(dialogSource).toContain("以后可在 设置 → 存储 中更改");
    expect(dialogSource).toContain("剩余");
  });

  it("pending 为 false 时不渲染任何东西", () => {
    expect(dialogSource).toMatch(/if \(!pending\) return null;/);
  });

  it("选默认位置：确认后关闭弹窗并回调 onDone", async () => {
    const confirmDefault = vi.fn(async () => undefined);
    const notify = vi.fn();
    const done = vi.fn();

    await useDefaultRootFlow({ confirmDefault, notify, done });

    expect(confirmDefault).toHaveBeenCalledTimes(1);
    expect(notify).not.toHaveBeenCalled();
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("选默认位置失败：提示错误且不关闭弹窗", async () => {
    const notify = vi.fn();
    const done = vi.fn();

    await useDefaultRootFlow({
      confirmDefault: async () => {
        throw new Error("磁盘只读");
      },
      notify,
      done,
    });

    expect(notify).toHaveBeenCalledWith(expect.stringContaining("磁盘只读"), "err");
    expect(done).not.toHaveBeenCalled();
  });

  it("选其他位置：选目录 → 迁移 → 关闭弹窗", async () => {
    const pickDir = vi.fn(async () => "D:\\我的数据");
    const move = vi.fn(async () => MOVE_REPORT);
    const notify = vi.fn();
    const done = vi.fn();

    await chooseOtherRootFlow({ pickDir, move, notify, done });

    expect(pickDir).toHaveBeenCalledTimes(1);
    expect(move).toHaveBeenCalledWith("D:\\我的数据");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("D:\\我的数据"));
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("选其他位置时取消选目录：不迁移、不关闭弹窗", async () => {
    const move = vi.fn(async () => MOVE_REPORT);
    const done = vi.fn();

    await chooseOtherRootFlow({
      pickDir: async () => null,
      move,
      notify: vi.fn(),
      done,
    });

    expect(move).not.toHaveBeenCalled();
    expect(done).not.toHaveBeenCalled();
  });
});
