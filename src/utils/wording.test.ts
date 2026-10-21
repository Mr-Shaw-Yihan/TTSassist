// 命名口径守卫（B7 / 决策 D7）。
// 为什么这条口径值得用测试钉住：收藏夹与克隆样本重录成本极高，一旦界面把用户内容说成
// 「缓存」，用户就会以为它和网页缓存一样可以随手清掉 —— 这是会真丢数据的文案事故。
// 权威定义见 ../doc/存储机制结构图.html：我的数据（珍贵资产）/ 临时文件（可清理）/ 配置。
//
// 环境说明：本项目 vitest 跑在 node 环境、未装 @testing-library/react，所以用 vite 的
// ?raw 把前端源码读成文本再判词，等价于对界面文案做快照。

import { describe, it, expect } from "vitest";

/** 前端全部源码（排除单测自身，否则本文件里的关键词会自己把自己判红） */
const rawSources = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const sources = Object.entries(rawSources).filter(([path]) => !/\.test\.tsx?$/.test(path));

/** 拼成一大段文本便于全局正则 */
const allText = sources.map(([, text]) => text).join("\n");

describe("用户内容命名口径", () => {
  it("源码文件确实被抓到（守卫本身没失效）", () => {
    expect(sources.length).toBeGreaterThan(30);
  });

  it("禁用「缓存」指代用户内容（消息/收藏/音频/样本/字幕历史/我的数据）", () => {
    const offender = /(?:消息|收藏|音频|样本|字幕历史|我的数据)[^#\n]{0,10}缓存|缓存[^#\n]{0,10}(?:消息|收藏|音频|样本|字幕历史|我的数据)/;
    const hits = sources
      .filter(([, text]) => offender.test(text))
      .map(([path]) => path.split("/").pop());
    expect(hits).toEqual([]);
  });

  it("「我的数据」这一叫法必须真的出现在界面文案里（不能只写在文档）", () => {
    expect(allText).toContain("我的数据");
  });

  it("删除类确认文案必须明说不会碰珍贵资产", () => {
    const orphans = sources.find(([path]) => path.endsWith("OrphanDataSection.tsx"));
    expect(orphans, "残留数据区块应仍存在").toBeTruthy();
    expect(orphans![1]).toContain("不会删除消息、收藏、音频与克隆样本");
  });

  it("残留引擎数据的删除按钮要写明删的是「引擎数据」，不留「删除数据」这种吓人的含糊说法", () => {
    const orphans = sources.find(([path]) => path.endsWith("OrphanDataSection.tsx"));
    expect(orphans![1]).toContain("删除引擎数据");
  });
});
