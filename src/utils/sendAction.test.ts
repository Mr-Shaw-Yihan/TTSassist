// 发送键点击决策的纯函数（置灰但仍可点、连点两次打断改发）。
// 需求（用户确认）：合成期间不阻塞输入，发送键置灰；第一次点=轻提示不打断，
// 第二次点=强制打断当前合成并改发最新一条。抽成纯函数便于单测钉死状态机。

import { describe, it, expect } from "vitest";
import { decideSendClick } from "./sendAction";

describe("decideSendClick", () => {
  it("空文本一律 noop（哪怕 busy/armed 也不该发送或打断）", () => {
    expect(decideSendClick({ busy: false, armed: false, hasText: false })).toBe("noop");
    expect(decideSendClick({ busy: true, armed: true, hasText: false })).toBe("noop");
  });

  it("空闲：直接发送", () => {
    expect(decideSendClick({ busy: false, armed: false, hasText: true })).toBe("send");
    expect(decideSendClick({ busy: false, armed: true, hasText: true })).toBe("send");
  });

  it("合成中且未武装：第一次点只 arm（提示），不打断", () => {
    expect(decideSendClick({ busy: true, armed: false, hasText: true })).toBe("arm");
  });

  it("合成中且已武装：第二次点 interrupt（打断改发）", () => {
    expect(decideSendClick({ busy: true, armed: true, hasText: true })).toBe("interrupt");
  });
});
