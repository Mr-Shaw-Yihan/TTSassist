import { describe, it, expect } from "vitest";
import { computeUpdateCount } from "./pluginUpdateStore";

const v = (id: string, version: string) => ({ id, version });

describe("computeUpdateCount（可更新插件计数）", () => {
  it("在线索引版本更高 → 计入", () => {
    expect(computeUpdateCount([v("mimo-tts", "0.1.0")], [], [v("mimo-tts", "0.2.0")])).toBe(1);
  });

  it("内置版本更高 → 计入", () => {
    expect(computeUpdateCount([v("edge-tts", "0.1.0")], [v("edge-tts", "0.1.1")], [])).toBe(1);
  });

  it("已装即最新（相等）→ 不计", () => {
    expect(computeUpdateCount([v("edge-tts", "0.1.0")], [], [v("edge-tts", "0.1.0")])).toBe(0);
  });

  it("取内置与在线中的最高版本比较", () => {
    expect(computeUpdateCount([v("a", "1.0.0")], [v("a", "1.1.0")], [v("a", "1.2.0")])).toBe(1);
    // 已装比候选都新 → 不计
    expect(computeUpdateCount([v("a", "2.0.0")], [v("a", "1.1.0")], [v("a", "1.2.0")])).toBe(0);
  });

  it("索引里有但未安装的插件 → 不计（只数已装的可更新）", () => {
    expect(computeUpdateCount([], [], [v("new-tts", "9.9.9")])).toBe(0);
  });

  it("多个已装、部分可更新 → 计数正确", () => {
    const installed = [v("a", "1.0.0"), v("b", "2.0.0"), v("c", "3.0.0")];
    const index = [v("a", "1.1.0"), v("b", "2.0.0"), v("c", "3.1.0")];
    expect(computeUpdateCount(installed, [], index)).toBe(2);
  });
});
