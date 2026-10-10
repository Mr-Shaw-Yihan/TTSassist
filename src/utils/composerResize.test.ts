// 输入区高度拖拽纯函数单测（分隔条拖拽改消息区/输入区大小）。
// 回归源头：App.tsx 旧实现把「累加高度」和「绝对锚点位移」混用，每次 pointermove
// 都把全程位移重复叠加到上一帧结果上 → 高度加速暴涨，拖一点即撞钳位边界。
// 本测试钉死「线性」正确行为，并复现旧「累加」写法的错误，防止再次回归。

import { describe, it, expect } from "vitest";
import { computeComposerHeight, COMPOSER_MIN } from "./composerResize";

const MAX = 500;

describe("computeComposerHeight", () => {
  it("向上拖（clientY 变小）按绝对位移线性增大高度", () => {
    // 起点 y=400、startH=172；指针移到 y=300（上移 100px）→ 高度 272
    expect(computeComposerHeight({ startH: 172, anchorY: 400, clientY: 300, min: COMPOSER_MIN, max: MAX })).toBe(272);
  });

  it("连续多帧移动不累积放大：位置相同则结果相同（旧累加写法的复现对比）", () => {
    // 正确：给定同一 clientY，无论经过多少中间帧，结果只取决于位移
    const a = computeComposerHeight({ startH: 172, anchorY: 400, clientY: 380, min: COMPOSER_MIN, max: MAX });
    const b = computeComposerHeight({ startH: 172, anchorY: 400, clientY: 380, min: COMPOSER_MIN, max: MAX });
    expect(a).toBe(b); // 上移 20px → 192，稳定不漂移
    expect(a).toBe(192);

    // 旧累加写法（模拟）：每帧把全程位移加到已覆盖的 h 上，会暴涨
    let buggyH = 172;
    for (const cy of [398, 396, 394, 380]) {
      buggyH = Math.min(MAX, Math.max(COMPOSER_MIN, buggyH + (400 - cy)));
    }
    expect(buggyH).toBeGreaterThan(a); // 明显大于正确值 192，正是「撞边界/跳变」的根因
  });

  it("向下拖（clientY 变大）缩小高度，但不低于下限", () => {
    expect(computeComposerHeight({ startH: 172, anchorY: 400, clientY: 500, min: COMPOSER_MIN, max: MAX })).toBe(COMPOSER_MIN);
  });

  it("向上拖超过上限时被钳到 max", () => {
    expect(computeComposerHeight({ startH: 172, anchorY: 400, clientY: 0, min: COMPOSER_MIN, max: MAX })).toBe(MAX);
  });

  it("零位移时保持起始高度不变", () => {
    expect(computeComposerHeight({ startH: 250, anchorY: 400, clientY: 400, min: COMPOSER_MIN, max: MAX })).toBe(250);
  });
});
