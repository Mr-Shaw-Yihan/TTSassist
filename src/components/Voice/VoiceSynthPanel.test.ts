// VoiceSynthPanel 验收：用户裁定（10-23）「保存的克隆音色排在音色列表顶部」。
//
// 10-23 第三轮真机复报暴露了更根本的问题：界面上其实有**两份**音色下拉
// ——走通用「音色管理」分区卡的，与走 voiceSelectNode 插槽的（带音色管理/专属面板的引擎）。
// 前两轮各修一处、漏掉另一处，用户看到的就是“一部分引擎分组了，另一部分没分组”。
// 因此分组判定抽成纯函数 classifyVoices，两条分支共用；本文件既测纯函数，
// 也用 ?raw 守卫钉住“每一处下拉都是我的音色在前”。
//
// 夹具一律用中性名（基座护栏不允许在本文件里新增引擎身份字面量）。

import { describe, expect, it } from "vitest";
import panelSource from "./VoiceSynthPanel.tsx?raw";
import { classifyVoices } from "./VoiceSynthPanel";

const V = (id: string, label = id) => ({ id, label });

describe("classifyVoices 我的音色归类", () => {
  it("宿主克隆音色 id=\"clone\" 归入我的音色，不留在内置组", () => {
    const r = classifyVoices([V("preset_a"), V("preset_b"), V("clone")], [], []);
    expect(r.mineFromBuiltIn.map((v) => v.id)).toEqual(["clone"]);
    expect(r.builtin.map((v) => v.id)).toEqual(["preset_a", "preset_b"]);
    expect(r.hasMine).toBe(true);
  });

  it("保存的自定义音色进我的音色，同 id 的内置项不重复出现在内置组", () => {
    const r = classifyVoices([V("saved_1"), V("preset_a")], [V("saved_1", "我的声音")], []);
    expect(r.custom.map((c) => c.id)).toEqual(["saved_1"]);
    expect(r.builtin.map((v) => v.id)).toEqual(["preset_a"]);
  });

  it("本地持久化过但云端列表查不到的克隆音色也算我的音色", () => {
    const r = classifyVoices([V("preset_a")], [], ["orphan_clone_1"]);
    expect(r.orphanClones).toEqual(["orphan_clone_1"]);
    expect(r.builtin.map((v) => v.id)).toEqual(["preset_a"]);
    expect(r.hasMine).toBe(true);
  });

  it("三类来源都没有时 hasMine=false（不渲染空的「我的音色」组）", () => {
    const r = classifyVoices([V("preset_a"), V("preset_b")], [], []);
    expect(r.hasMine).toBe(false);
    expect(r.builtin.map((v) => v.id)).toEqual(["preset_a", "preset_b"]);
  });
});

describe("两份音色下拉都分组且「我的音色」置顶", () => {
  it("每一处 optgroup 配对都是「我的音色」在「内置音色」之前", () => {
    const mine = [...panelSource.matchAll(/<optgroup label="我的音色">/g)].map((m) => m.index as number);
    const builtin = [...panelSource.matchAll(/<optgroup label="内置音色">/g)].map((m) => m.index as number);
    // 至少两份下拉：通用「音色管理」分区卡 + voiceSelectNode
    expect(mine.length).toBeGreaterThanOrEqual(2);
    expect(builtin.length).toBe(mine.length);
    // 按源码顺序成对比较（渲染顺序即源码顺序）
    for (let i = 0; i < mine.length; i++) {
      expect(mine[i]).toBeLessThan(builtin[i]);
    }
  });

  it("分组只算一次、两份下拉都从 grouped 渲染（防止某处绕开它自己判）", () => {
    // classifyVoices 定义 1 处 + 调用 1 处（算出 grouped 供两份下拉共用）
    expect([...panelSource.matchAll(/classifyVoices\(/g)].length).toBeGreaterThanOrEqual(2);
    // 两份下拉各自的「我的音色」开关与内置组数据源都必须来自 grouped
    expect([...panelSource.matchAll(/grouped\.hasMine &&/g)].length).toBeGreaterThanOrEqual(2);
    expect([...panelSource.matchAll(/grouped\.builtin\.map/g)].length).toBeGreaterThanOrEqual(2);
    // 内置组不得直接拿 cur.voices 渲染（那就又把 clone 混回内置了）
    expect(panelSource).not.toMatch(/<optgroup label="内置音色">[\s\S]{0,120}cur\.voices\.map/);
  });
});
