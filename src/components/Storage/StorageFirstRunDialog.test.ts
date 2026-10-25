// 首启位置选择弹窗验收：vitest 为 node 环境且无 testing-library，
// 渲染分支用 ?raw 源码快照钉住，交互走导出的纯编排函数。

import { describe, expect, it, vi } from "vitest";
import dialogSource from "./StorageFirstRunDialog.tsx?raw";
import { chooseOtherRootFlow, firstRunGateFlow, useDefaultRootFlow, withStartupRetry } from "./StorageFirstRunDialog";
import type { StorageMoveReport, StorageReport } from "../../services/invoke";

const MOVE_REPORT: StorageMoveReport = {
  moved_bytes: 0,
  moved_files: 0,
  skipped: [],
  backup_dir: "",
  need_restart: false,
};

// firstRunGateFlow 只把 report 透传给 setReport，不读其字段，故用最小桩即可
const REPORT = { content_root: "C:\\x", drives: [], buckets: [] } as unknown as StorageReport;

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

    await chooseOtherRootFlow({
      pickDir,
      move,
      notify,
      noticeRestart: vi.fn(async () => {}),
      restart: vi.fn(async () => {}),
      done,
    });

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
      noticeRestart: vi.fn(async () => {}),
      restart: vi.fn(async () => {}),
      done,
    });

    expect(move).not.toHaveBeenCalled();
    expect(done).not.toHaveBeenCalled();
  });

  // 首启弹窗里选了别的盘同样要重启：不重启就仍读写旧位置，那期间新发的消息会“消失”。
  it("选其他位置且需重启：告知完直接重启，不收“稍后再说”", async () => {
    const noticeRestart = vi.fn(async (message: string) => {
      void message;
    });
    const restart = vi.fn(async () => {});
    const done = vi.fn();

    await chooseOtherRootFlow({
      pickDir: async () => "D:\\我的数据",
      move: async () => ({ ...MOVE_REPORT, need_restart: true }),
      notify: vi.fn(),
      noticeRestart,
      restart,
      done,
    });

    expect(noticeRestart).toHaveBeenCalledTimes(1);
    expect(noticeRestart.mock.calls[0][0]).toContain("D:\\我的数据");
    expect(noticeRestart.mock.calls[0][0]).toContain("重启");
    expect(restart).toHaveBeenCalledTimes(1);
    // 进程即将退出，不再走“关闭弹窗 + 回调”那套
    expect(done).not.toHaveBeenCalled();
  });

  it("重启告知不抢跑：用模态框等用户点，不用超时 race（真机实测 5 秒太快）", () => {
    expect(dialogSource).not.toContain("Promise.race");
    expect(dialogSource).toContain('confirmText: "立即重启"');
  });
});

// 根因回归（10-23）：真机首启弹窗不出现。旧实现把「弹窗可见性」与 getStorageReport()
// 绑在同一个 try 里，启动早期 report 一旦 reject 就被静默 catch 吞掉 → pending 停在不显示。
// 修复后：可见性只由 pending()（storage_choice_pending）决定，report 失败/慢都照弹、只降级显示。
describe("firstRunGateFlow 门禁与体积解耦", () => {
  it("需要弹且体积读取成功：亮出弹窗并填入 report", async () => {
    const show = vi.fn(), hide = vi.fn(), setReport = vi.fn(), setError = vi.fn();
    await firstRunGateFlow({ pending: async () => true, report: async () => REPORT, show, hide, setReport, setError });
    expect(show).toHaveBeenCalledTimes(1);
    expect(setReport).toHaveBeenCalledWith(REPORT);
    expect(hide).not.toHaveBeenCalled();
    expect(setError).not.toHaveBeenCalled();
  });

  it("已确认过位置（pending=false）：不亮弹窗", async () => {
    const show = vi.fn(), hide = vi.fn(), setReport = vi.fn(), setError = vi.fn();
    await firstRunGateFlow({ pending: async () => false, report: async () => REPORT, show, hide, setReport, setError });
    expect(hide).toHaveBeenCalledTimes(1);
    expect(show).not.toHaveBeenCalled();
    expect(setReport).not.toHaveBeenCalled();
  });

  it("【回归】体积读取失败也必须亮出弹窗（可见性不被 report 绑架）并显示错误", async () => {
    const show = vi.fn(), hide = vi.fn(), setReport = vi.fn(), setError = vi.fn();
    await firstRunGateFlow({
      pending: async () => true,
      report: async () => { throw new Error("启动早期状态未就绪"); },
      show, hide, setReport, setError,
    });
    expect(show).toHaveBeenCalledTimes(1);
    expect(hide).not.toHaveBeenCalled();
    expect(setReport).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledWith(expect.stringContaining("启动早期状态未就绪"));
  });

  it("门禁读取失败：保守亮出弹窗并显示错误（宁可多问不可漏问）", async () => {
    const show = vi.fn(), hide = vi.fn(), setReport = vi.fn(), setError = vi.fn();
    await firstRunGateFlow({
      pending: async () => { throw new Error("命令未注册"); },
      report: async () => REPORT,
      show, hide, setReport, setError,
    });
    expect(show).toHaveBeenCalledTimes(1);
    expect(setError).toHaveBeenCalledWith(expect.stringContaining("命令未注册"));
    expect(hide).not.toHaveBeenCalled();
  });
});

// 启动竞态守卫（10-23 真机实钳）：后端 setup 里 AppState 是「刻意晚 manage」的
//（搬家期间禁止前端写内容根，见 lib.rs 注释），而首启弹窗的 IPC 发得最早，
// 撞上 “state not managed” 属正常瞬态 → 必须延迟重试一次，不能一次失败就降级。
describe("withStartupRetry 启动瞬态重试", () => {
  it("首次就成功：不重试、不等待", async () => {
    const fn = vi.fn(async () => 42);
    const sleep = vi.fn(async () => {});
    await expect(withStartupRetry(fn, 800, sleep)).resolves.toBe(42);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(sleep).not.toHaveBeenCalled();
  });

  it("首次失败、重试成功：调用两次并返回重试结果", async () => {
    let n = 0;
    const fn = vi.fn(async () => {
      n += 1;
      if (n === 1) throw new Error("state not managed");
      return "ok";
    });
    const sleep = vi.fn(async () => {});
    await expect(withStartupRetry(fn, 800, sleep)).resolves.toBe("ok");
    expect(fn).toHaveBeenCalledTimes(2);
    expect(sleep).toHaveBeenCalledWith(800);
  });

  it("两次都失败：把第二次的错误抛出（由调用方降级显示）", async () => {
    const fn = vi.fn(async () => {
      throw new Error("存储路径尚未就绪");
    });
    const sleep = vi.fn(async () => {});
    await expect(withStartupRetry(fn, 800, sleep)).rejects.toThrow("存储路径尚未就绪");
    expect(fn).toHaveBeenCalledTimes(2);
  });
});
