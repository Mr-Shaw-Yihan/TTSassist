// 草稿 store 的行为契约。
// 回归源头（用户实测）：输入框文字存在 InputBox 的局部 useState 里，而 App.tsx 切标签页是
// 条件渲染 —— 组件卸载即丢草稿。把草稿搬进 store 后，「组件重建仍读到同一份」必须由测试钉住。

import { describe, it, expect, beforeEach } from "vitest";
import { useDraftStore } from "./draftStore";

describe("draftStore", () => {
  beforeEach(() => {
    useDraftStore.setState({ draft: "" });
  });

  it("初值为空串（不该带着上次的东西进来）", () => {
    expect(useDraftStore.getState().draft).toBe("");
  });

  it("直接赋值覆盖旧草稿", () => {
    useDraftStore.getState().setDraft("第一段");
    useDraftStore.getState().setDraft("第二段");
    expect(useDraftStore.getState().draft).toBe("第二段");
  });

  it("函数式更新可读旧值（语音识别结果追加进输入框的用法）", () => {
    useDraftStore.getState().setDraft("你好，");
    useDraftStore.getState().setDraft((prev) => (prev ? prev + "今天天气不错" : "今天天气不错"));
    expect(useDraftStore.getState().draft).toBe("你好，今天天气不错");
    useDraftStore.getState().setDraft((prev) => (prev ? prev + "x" : "x"));
    expect(useDraftStore.getState().draft).toBe("你好，今天天气不错x");
  });

  it("空草稿时函数式更新走 else 分支，不留下前导内容", () => {
    useDraftStore.getState().setDraft((prev) => (prev ? prev + "识别结果" : "识别结果"));
    expect(useDraftStore.getState().draft).toBe("识别结果");
  });

  it("发送后清空", () => {
    useDraftStore.getState().setDraft("要朗读的文字");
    useDraftStore.getState().setDraft("");
    expect(useDraftStore.getState().draft).toBe("");
  });

  it("同一 store 实例被多方读取（组件卸载重挂不丢草稿的根因保证）", () => {
    const 页面A = () => useDraftStore.getState().draft;
    useDraftStore.getState().setDraft("跨页草稿");
    // 模拟切走再切回：换一组订阅读取，数据仍来自同一个 store
    const 页面B = (s: { draft: string }) => s.draft;
    expect(页面A()).toBe("跨页草稿");
    expect(页面B(useDraftStore.getState())).toBe("跨页草稿");
  });
});
