// VoiceSynthPanel 验收：用户裁定（10-23）「保存的克隆音色排在音色列表顶部」
// 要对所有走通用音色下拉的插件生效（不只 minimax）。
// node 环境无 testing-library，故用 ?raw 源码守卫钉住两个 optgroup 的先后顺序。

import { describe, expect, it } from "vitest";
import panelSource from "./VoiceSynthPanel.tsx?raw";

describe("VoiceSynthPanel 音色下拉顺序", () => {
  it("「我的音色」组在「内置音色」组之前（保存的克隆音色排顶部）", () => {
    const mine = panelSource.indexOf('<optgroup label="我的音色">');
    const builtin = panelSource.indexOf('<optgroup label="内置音色">');
    expect(mine).toBeGreaterThan(-1);
    expect(builtin).toBeGreaterThan(-1);
    expect(mine).toBeLessThan(builtin);
  });
});
