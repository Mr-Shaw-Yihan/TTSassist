// 首帧皮肤判定单测（B2：启动即应用记忆皮肤）。
// 回归源头：index.html 不写 data-theme，真实主题要等 settings.json 异步加载完由
// App.tsx 的 effect 补上 → 夜间用户每次启动都先闪一帧日间皮肤。
// 修法依赖两段代码读同一个 localStorage 键（本文件 + index.html 内联同步脚本），
// 所以这里同时钉住常量取值与内联脚本的存在，防止任一侧被改动后静默失效。

import { describe, it, expect } from "vitest";
import { resolveInitialTheme, THEME_STORAGE_KEY } from "./theme";
// 用 vite 的 ?raw 读启动页（本项目的 tsconfig 不含 node 类型，不能 import node:fs）
import indexHtml from "../../index.html?raw";

describe("resolveInitialTheme", () => {
  it("持久化值为 dark 时判定 dark", () => {
    expect(resolveInitialTheme("dark")).toBe("dark");
  });

  it("显式 light 时判定 light", () => {
    expect(resolveInitialTheme("light")).toBe("light");
  });

  it("首次启动没有任何持久化值时判定 light", () => {
    expect(resolveInitialTheme(null)).toBe("light");
    expect(resolveInitialTheme(undefined)).toBe("light");
    expect(resolveInitialTheme("")).toBe("light");
  });

  it("持久化内容是坏 JSON 时判定 light（坏数据不得让皮肤逻辑抛错）", () => {
    expect(resolveInitialTheme("{ 坏 JSON,")).toBe("light");
  });

  it("持久化内容是 JSON 但缺 theme 字段时判定 light", () => {
    expect(resolveInitialTheme('{"other":"dark"}')).toBe("light");
  });

  it("持久化内容是带 theme 字段的 JSON 时按该字段判定", () => {
    expect(resolveInitialTheme('{"theme":"dark"}')).toBe("dark");
  });

  it("无法识别的取值一律回落 light", () => {
    expect(resolveInitialTheme("DARK")).toBe("light");
    expect(resolveInitialTheme("night")).toBe("light");
  });
});

describe("皮肤持久化键与启动引导脚本", () => {
  it("键名为 va-theme", () => {
    expect(THEME_STORAGE_KEY).toBe("va-theme");
  });

  it("index.html 内联脚本同步读取该键并设置 data-theme", () => {
    expect(indexHtml).toContain(THEME_STORAGE_KEY);
    expect(indexHtml).toContain('setAttribute("data-theme"');
    // 必须是同步脚本：带 type=module 会被推迟到解析后执行，首帧仍会闪
    expect(indexHtml).not.toMatch(/<script[^>]+type="module"[^>]*>[\s\S]*data-theme/);
  });
});
