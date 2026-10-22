// MinimaxVoicePanel 验收：用户裁定（10-23）「保存的克隆音色排在音色管理列表顶部」。
// node 环境无 testing-library，故对抽出的纯函数做行为测试。

import { describe, expect, it } from "vitest";
import { sortSavedVoicesFirst } from "./MinimaxVoicePanel";

const LIST = [
  { voice_id: "a", created_time: "1" },
  { voice_id: "saved-2", created_time: "2" },
  { voice_id: "b", created_time: "3" },
  { voice_id: "saved-1", created_time: "4" },
];

describe("sortSavedVoicesFirst", () => {
  it("本应用保存过的克隆音色排到顶部，组内与其余保持原序（稳定排序）", () => {
    const sorted = sortSavedVoicesFirst(LIST, ["saved-1", "saved-2"]);
    expect(sorted.map((v) => v.voice_id)).toEqual(["saved-2", "saved-1", "a", "b"]);
  });

  it("没有保存项时保持原序", () => {
    expect(sortSavedVoicesFirst(LIST, []).map((v) => v.voice_id)).toEqual([
      "a",
      "saved-2",
      "b",
      "saved-1",
    ]);
  });

  it("保存项不在账号列表里时不影响顺序", () => {
    expect(sortSavedVoicesFirst(LIST, ["ghost"]).map((v) => v.voice_id)).toEqual([
      "a",
      "saved-2",
      "b",
      "saved-1",
    ]);
  });

  it("不修改入参数组", () => {
    const copy = [...LIST];
    sortSavedVoicesFirst(LIST, ["saved-1"]);
    expect(LIST).toEqual(copy);
  });
});
