// 占位消息状态机的行为契约。
// 回归源头（用户真机报出）：流式是「先出声、后落库」，若前端等命令返回才入列，
// 用户在听到声音后既看不到这条消息、也没有任何可点的停止入口。

import { describe, it, expect } from "vitest";
import {
  withStreamingPlaceholder,
  settleStreaming,
  dropStreaming,
  isStreamingPlaceholder,
  withId,
  withoutId,
  isFailed,
  appendPlaceholder,
  replacePlaceholder,
  markFailed,
  clearFailed,
  type StreamingNotice,
} from "./streamPlaceholder";
import type { Message } from "../types";

const notice = (over: Partial<StreamingNotice> = {}): StreamingNotice => ({
  id: "m_1",
  content: "你好，这是流式合成",
  audio_path: "audio/m_1.wav",
  engine: "demo-tts", // 中立占位：不往基座测试里带真实引擎名（基座护栏）
  created_at: "2026-10-11T01:19:47+08:00",
  ...over,
});

const msg = (over: Partial<Message> = {}): Message => ({
  id: "m_1",
  content: "你好，这是流式合成",
  audio_path: "audio/m_1.wav",
  created_at: "2026-10-11T01:19:51+08:00",
  ...over,
});

const base: Message[] = [msg({ id: "m_old", content: "上一条" })];

describe("withStreamingPlaceholder", () => {
  it("占位消息追加到末尾，字段取自通知", () => {
    const out = withStreamingPlaceholder(base, notice());
    expect(out).toHaveLength(2);
    expect(out[1]).toEqual({
      id: "m_1",
      content: "你好，这是流式合成",
      audio_path: "audio/m_1.wav",
      created_at: "2026-10-11T01:19:47+08:00",
    });
    // 不动已有条目
    expect(out[0].id).toBe("m_old");
  });

  it("同一 id 重复通知保持幂等（不出现两条一样的消息）", () => {
    const once = withStreamingPlaceholder(base, notice());
    const twice = withStreamingPlaceholder(once, notice());
    expect(twice).toBe(once);
    expect(twice.filter((m) => m.id === "m_1")).toHaveLength(1);
  });

  it("不修改传入的列表（React state 更新依赖新数组）", () => {
    withStreamingPlaceholder(base, notice());
    expect(base).toHaveLength(1);
  });
});

describe("settleStreaming", () => {
  it("按 id 原地替换占位，位置不变（播完那一刻消息不跳位）", () => {
    const withPh = withStreamingPlaceholder(base, notice());
    const out = settleStreaming(withPh, msg({ created_at: "2026-10-11T01:19:51+08:00" }));
    expect(out).toHaveLength(2);
    expect(out[1].created_at).toBe("2026-10-11T01:19:51+08:00");
    expect(out[0].id).toBe("m_old");
  });

  it("通知丢失/时序倒置（列表里还没有占位）时追加，绝不丢消息", () => {
    const out = settleStreaming(base, msg());
    expect(out).toHaveLength(2);
    expect(out[1].id).toBe("m_1");
  });

  it("替换后不再残留占位字段", () => {
    const withPh = withStreamingPlaceholder(base, notice());
    const out = settleStreaming(withPh, msg({ content: "最终内容" }));
    expect(out[1].content).toBe("最终内容");
  });
});

describe("dropStreaming", () => {
  it("失败时按 id 摘除占位，其它条目不动（宁缺毋残）", () => {
    const withPh = withStreamingPlaceholder(base, notice());
    const out = dropStreaming(withPh, "m_1");
    expect(out.map((m) => m.id)).toEqual(["m_old"]);
  });

  it("id 不存在时是无害操作", () => {
    expect(dropStreaming(base, "m_none")).toEqual(base);
  });
});

describe("isStreamingPlaceholder", () => {
  it("在 streaming 集合里的才算占位", () => {
    expect(isStreamingPlaceholder(["m_1"], msg())).toBe(true);
    expect(isStreamingPlaceholder([], msg())).toBe(false);
    expect(isStreamingPlaceholder(["m_other"], msg())).toBe(false);
  });
});

describe("集合增删与 isFailed", () => {
  it("withId 幂等、withoutId 不含时无害", () => {
    expect(withId(["a"], "a")).toEqual(["a"]);
    expect(withId(["a"], "b")).toEqual(["a", "b"]);
    expect(withoutId(["a", "b"], "a")).toEqual(["b"]);
    expect(withoutId(["a"], "z")).toEqual(["a"]);
  });
  it("isFailed 看 failedIds", () => {
    expect(isFailed(["m_1"], msg())).toBe(true);
    expect(isFailed([], msg())).toBe(false);
  });
});

describe("appendPlaceholder / replacePlaceholder（阻塞引擎 temp 占位）", () => {
  it("抢先插入不重复", () => {
    const once = appendPlaceholder(base, msg({ id: "tmp" }));
    const twice = appendPlaceholder(once, msg({ id: "tmp" }));
    expect(twice).toBe(once);
    expect(twice.filter((m) => m.id === "tmp")).toHaveLength(1);
  });
  it("按位置把 temp 换成正式消息（id 变、位置不变）", () => {
    const withTmp = appendPlaceholder(base, msg({ id: "tmp", content: "待合成" }));
    const out = replacePlaceholder(withTmp, "tmp", msg({ id: "real", content: "最终" }));
    expect(out.map((m) => m.id)).toEqual(["m_old", "real"]);
    expect(out[1].content).toBe("最终");
  });
  it("temp 不在时退化为按 id 替换/追加，不丢消息", () => {
    const out = replacePlaceholder(base, "tmp", msg({ id: "m_new" }));
    expect(out.map((m) => m.id)).toEqual(["m_old", "m_new"]);
  });
});

describe("markFailed / clearFailed（失败气泡保留可重试）", () => {
  it("流式占位行已在：移到 failed、从 streaming 摘掉、行不丢", () => {
    const list = withStreamingPlaceholder(base, notice({ id: "m_1" }));
    const r = markFailed(list, ["m_1"], [], { id: "m_1", content: "x", created_at: "t" });
    expect(r.streamingIds).toEqual([]);
    expect(r.failedIds).toEqual(["m_1"]);
    expect(r.list.some((m) => m.id === "m_1")).toBe(true); // 行保留
  });
  it("阻塞无占位行时补插一行（audio_path 为空）", () => {
    const r = markFailed(base, [], [], { id: "m_x", content: "发不出去", created_at: "t" });
    expect(r.failedIds).toEqual(["m_x"]);
    expect(r.list[1]).toEqual({ id: "m_x", content: "发不出去", created_at: "t", audio_path: "" });
  });
  it("clearFailed 按 id 摘除行与 failed 标记", () => {
    const withFailed = markFailed(base, [], ["m_x"], { id: "m_x", content: "y", created_at: "t" });
    const c = clearFailed(withFailed.list, withFailed.failedIds, "m_x");
    expect(c.list.some((m) => m.id === "m_x")).toBe(false);
    expect(c.failedIds).toEqual([]);
  });
});
