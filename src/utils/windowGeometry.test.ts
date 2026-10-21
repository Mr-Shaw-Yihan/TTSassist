// 窗口尺寸记忆改造（B3，配合 A10）。
// 分工变化：尺寸改由 Rust 在**显示之前**按 settings.json 的记忆值应用（window_width /
// window_height，0 = 无记忆，并夹到 [最小尺寸, 屏幕尺寸]），前端只负责**保存**，不再在
// 启动后补一次 setSize —— 那一次正是「先竖版露脸、再横向弹开」跳动的来源。
// 位置与最大化仍留在 localStorage（A10 未迁，本任务只管尺寸）。
//
// 环境说明：vitest 是 node 环境、无 @testing-library/react，无法挂载 hook 断言 setSize
// 调用次数，故把可判定的部分抽成纯函数单测，并对 hook 源码做结构性守卫（不许出现 setSize）。

import { describe, it, expect } from "vitest";
import { MIN_SIDE_PHYSICAL, plausibleSize, windowSizeSettings } from "./windowGeometry";
import useWindowStateSrc from "../hooks/useWindowState.ts?raw";

describe("plausibleSize：占位/退化尺寸必须拦下", () => {
  it("主窗是 visible=false 创建，未布局时读到的 1×1 类小值不能记", () => {
    expect(plausibleSize(1, 1)).toBe(false);
    expect(plausibleSize(0, 0)).toBe(false);
  });

  it("任一边低于下限就整体不记（半截尺寸记回去更糟）", () => {
    expect(plausibleSize(MIN_SIDE_PHYSICAL - 1, 1200)).toBe(false);
    expect(plausibleSize(1200, MIN_SIDE_PHYSICAL - 1)).toBe(false);
  });

  it("恰好等于下限与正常尺寸都算合理", () => {
    expect(plausibleSize(MIN_SIDE_PHYSICAL, MIN_SIDE_PHYSICAL)).toBe(true);
    expect(plausibleSize(1280, 800)).toBe(true);
  });
});

describe("windowSizeSettings：写进 settings 的两个键", () => {
  it("合理尺寸产出 window_width / window_height（物理像素原值，夹取由 Rust 负责）", () => {
    expect(windowSizeSettings(1440, 900)).toEqual([
      ["window_width", 1440],
      ["window_height", 900],
    ]);
  });

  it("占位尺寸不产出任何写入（宁可保持上次记忆，也不覆盖成小窗）", () => {
    expect(windowSizeSettings(1, 1)).toEqual([]);
  });
});

describe("useWindowState 的职责收缩", () => {
  it("不再调用 setSize（首帧尺寸归 Rust）", () => {
    expect(useWindowStateSrc).not.toContain("setSize(");
  });

  it("改为把尺寸写进 settings", () => {
    expect(useWindowStateSrc).toContain("windowSizeSettings(");
    expect(useWindowStateSrc).toContain("updateSetting");
  });

  it("位置与最大化记忆仍留在 localStorage", () => {
    expect(useWindowStateSrc).toContain("setPosition(");
    expect(useWindowStateSrc).toContain("maximize()");
  });
});
