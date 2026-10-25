// 首帧皮肤守卫（10-23 第二轮真机复报）。
// 现象：窗口出现后内容先是浅色、约 1 秒才转成夜间（ComputerUse 逐帧采样实证：
// 空窗阶段已是深色底，内容绘制出来却是浅色）。
// 根因不在 index.html 的内联脚本，而在 App 自己：主题按 `settings?.theme === "dark"`
// 计算，settings 还没异步读回来时是 null → 算出 "light" → effect 把内联脚本已经定好的
// 夜间首帧**覆盖**成日间，等 getSettings() 返回才翻回来。
// 所以规矩是：settings 未加载完，一律不碰 data-theme（首帧交给内联脚本 + localStorage 镜像）。
//
// 环境说明：vitest 是 node 环境、无 @testing-library/react，无法渲染 App 断言换肤时机，
// 故对 App.tsx 源码做结构守卫（写 data-theme 之前必须已有 null 守卫）。

import { describe, it, expect } from "vitest";
import appSrc from "./App.tsx?raw";

describe("App.tsx 首帧皮肤不被未加载完的设置覆盖", () => {
  it("写 data-theme 之前有「settings 为 null 就直接返回」的守卫", () => {
    const write = appSrc.indexOf(
      'document.documentElement.setAttribute("data-theme", theme)',
    );
    expect(write).toBeGreaterThan(-1);
    const guard = appSrc.lastIndexOf("if (settings === null) return;", write);
    expect(guard).toBeGreaterThan(-1);
    // 守卫必须紧贴写入（同一个 effect 内），隔太远说明它管不到这次写入
    expect(write - guard).toBeLessThan(400);
  });
});
