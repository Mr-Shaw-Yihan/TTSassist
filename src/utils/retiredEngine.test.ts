// 本地引擎下架后的前端残留守卫（B6 / 决策 D1、D12）。
// 两个本地引擎插件的 crate、预装 zip 与索引条目由 A 在 A6 删除，前端这一半归 B：
// 只留云端引擎，环境安装/离线资源包这类只有本地引擎才需要的界面与类型一并撤掉。
// 留着它们不只是脏：界面会摆出点了必然失败的按钮，类型里也继续要求宿主提供已删的命令。
//
// invoke.ts 里的本地引擎命令封装由 A 删除（该文件 A 独占，B 不动），故扫描排除它。

import { describe, it, expect } from "vitest";

const SELF = "retiredEngine.test.ts";

/** 已下架引擎的身份字面量（这些 id 只许出现在本行，别处一律算残留） */
// baseok: 本守卫必须字面写出这些 id 才能扫出别人的残留，属检测器而非基座写死
const FORBIDDEN_WORDS = ["genie", "hojo", "GenieData", "离线资源包"];

/** 随本地引擎一起消失的命令封装名 */
const FORBIDDEN_SYMBOLS = [
  "importResourcePackFlow",
  "importOfflineResources",
  "cleanFailedResources",
  "pickResourcePackZip",
];

const rawSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const targets = Object.entries(rawSources).filter(
  ([path]) => !path.endsWith(SELF) && !path.endsWith("services/invoke.ts"),
);

function findHits(needles: string[]): string[] {
  const bad: string[] = [];
  for (const [path, text] of targets) {
    const hit = needles.find((w) => text.includes(w));
    if (hit) bad.push(`${path.replace(/^\.\.\//, "")} → ${hit}`);
  }
  return bad.sort();
}

describe("本地引擎前端残留", () => {
  it("扫描范围覆盖到前端源码（守卫没空转）", () => {
    expect(targets.length).toBeGreaterThan(30);
  });

  it("src 下不再有已下架引擎的身份字面量与离线资源包字样", () => {
    expect(findHits(FORBIDDEN_WORDS)).toEqual([]);
  });

  it("src 下不再引用已删除的本地引擎命令封装", () => {
    expect(findHits(FORBIDDEN_SYMBOLS)).toEqual([]);
  });
});
