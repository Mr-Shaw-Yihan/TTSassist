// 设置页分区顺序（B4：快捷键前置为第一项）。
// 用户问题来源：设置页是一整页手风琴，快捷键原本排在「虚拟麦克风 / 插件服务 / 存储」之后，
// 最常用的键盘配置要往下翻才找得到。
//
// 为什么断言源码顺序而不是渲染结果：本仓库 vitest 跑在 node 环境且未安装
// @testing-library/react，无法挂载组件；而 SettingsPage 的分区是写死的静态 JSX 列表，
// 渲染顺序即源码顺序 —— 有人把分区挪回去或插到它前面，这里立刻红。

import { describe, it, expect } from "vitest";
import settingsPageSrc from "./SettingsPage.tsx?raw";

/** 按出现顺序取出每个 <Section> 的标题 */
function sectionTitlesInOrder(src: string): string[] {
  const titles: string[] = [];
  for (const block of src.split("<Section").slice(1)) {
    const literal = block.match(/title="([^"]+)"/);
    if (literal) {
      titles.push(literal[1]);
      continue;
    }
    // 「关于」的标题是 JSX 表达式（要挂更新红点），取表达式里第一个中文词
    const expr = block.match(/title=\{[\s\S]*?([一-龥]{2,4})/);
    titles.push(expr ? expr[1] : "");
  }
  return titles;
}

describe("设置页分区顺序", () => {
  it("分区标题按源码顺序全部取出（5 个，无一漏网）", () => {
    expect(sectionTitlesInOrder(settingsPageSrc)).toHaveLength(5);
  });

  it("「快捷键」是第一个分区", () => {
    expect(sectionTitlesInOrder(settingsPageSrc)[0]).toBe("快捷键");
  });

  it("「关于」仍排在最后（诊断与联系方式不该跑到前面）", () => {
    const titles = sectionTitlesInOrder(settingsPageSrc);
    expect(titles[titles.length - 1]).toBe("关于");
  });
});
