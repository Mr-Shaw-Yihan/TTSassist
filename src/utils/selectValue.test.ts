// resolveSelectedValue 的行为契约。
// 回归源头（用户实测）：音色库里只有一个条目时，下拉的 value 是空串、选项里没有匹配项，
// 点那唯一一项不触发 change，选中动作彻底失效；加到两个音色后再选才生效。
// 本测试把「单音色也必须给出可用选中值」钉死，防止再次回归。

import { describe, it, expect } from "vitest";
import { resolveSelectedValue } from "./selectValue";

const ONLY = ["afbe646a"];

describe("resolveSelectedValue", () => {
  it("只有一个选项且未存值时，回落到该选项（用户实测的失效场景）", () => {
    expect(resolveSelectedValue("", ONLY)).toBe("afbe646a");
    expect(resolveSelectedValue(undefined, ONLY)).toBe("afbe646a");
    expect(resolveSelectedValue(null, ONLY)).toBe("afbe646a");
  });

  it("存的值仍在选项里时原样返回，绝不擅自改选", () => {
    expect(resolveSelectedValue("b", ["a", "b", "c"])).toBe("b");
  });

  it("存的值是脏数据（音色已删除）时回落第一项", () => {
    expect(resolveSelectedValue("deleted-id", ["a", "b"])).toBe("a");
  });

  it("空选项集返回空串（此时调用方应提示『先添加音色』而不是选中某个）", () => {
    expect(resolveSelectedValue("a", [])).toBe("");
    expect(resolveSelectedValue("a", ["", "   ", undefined])).toBe("");
  });

  it("两侧空白都参与比较，纯空白存储值算未选", () => {
    expect(resolveSelectedValue("  a  ", ["a"])).toBe("a");
    expect(resolveSelectedValue("   ", ["a", "b"])).toBe("a");
  });

  it("选项自身的空白被剔除后不误当有效项，带空白的合法值仍可用", () => {
    expect(resolveSelectedValue("", ["  ", "b"])).toBe("b");
    expect(resolveSelectedValue("b", ["b ", "c"])).toBe("b");
  });
});
