// 消息列表 + 合成态（streaming / failed）的收敛状态机。
//
// 为什么搬进 store：非阻塞发送要求「点发送立刻进气泡、解放输入框」，一条气泡会跨越
// 三个异步来源（本窗 handleSend 抢先插的 temp 占位、后端 tts:streaming 真实 id、
// generateTTS 返回值 / tts:failed 事件）。若这些状态散在 App 的多个 useState 里，
// 异步回调拿到的是过期快照，会出现「temp 与真实 id 两条气泡并存」「失败气泡被翻页合并
// 挤到列表顶部」等竞态。store 的 action 通过 get()/set() 读写同一份快照，天然原子，
// 也让转移可被单测钉死（对齐计划 Task 5「或用 store 测试」）。

import { describe, it, expect, beforeEach } from "vitest";
import { useChatStore } from "./chatStore";
import type { Message } from "../types";
import type { StreamingNotice } from "../utils/streamPlaceholder";

const msg = (over: Partial<Message> = {}): Message => ({
  id: "m_real",
  content: "你好",
  audio_path: "audio/m_real.wav",
  created_at: "2026-10-13T10:00:00+08:00",
  ...over,
});

const notice = (over: Partial<StreamingNotice> = {}): StreamingNotice => ({
  id: "m_real",
  content: "你好",
  audio_path: "audio/m_real.wav",
  engine: "demo-tts", // 中立占位：不往基座测试带真实引擎名（基座护栏）
  created_at: "2026-10-13T10:00:00+08:00",
  ...over,
});

describe("chatStore 非阻塞发送状态机", () => {
  beforeEach(() => {
    useChatStore.setState({
      messages: [],
      streamingIds: [],
      failedIds: [],
      reasons: {},
      hasMore: false,
    });
  });

  it("beginPlaceholder：发送即抢先插入 temp 占位并标为合成中", () => {
    useChatStore.getState().beginPlaceholder(msg({ id: "tmp1", audio_path: "" }));
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["tmp1"]);
    expect(s.streamingIds).toContain("tmp1");
    expect(s.failedIds).toEqual([]);
  });

  it("流式成功：adoptStreaming 把 temp 原地升级为真实 id（只有一条气泡）", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", content: "你好", audio_path: "" }));
    st.adoptStreaming(notice({ id: "m_real", content: "你好" }));
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["m_real"]); // temp 被换掉，不重复
    expect(s.streamingIds).toContain("m_real");
    expect(s.streamingIds).not.toContain("tmp1");
  });

  it("跨窗口流式：无本地 temp 时 adoptStreaming 直接追加真实气泡", () => {
    useChatStore.getState().adoptStreaming(notice({ id: "m_real" }));
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["m_real"]);
    expect(s.streamingIds).toContain("m_real");
  });

  it("阻塞成功：settle 用真实消息按位置替换 temp（无流式事件也要落对）", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", content: "你好", audio_path: "" }));
    st.settle("tmp1", msg({ id: "m_real", content: "你好" }));
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["m_real"]);
    expect(s.messages[0].audio_path).toBe("audio/m_real.wav");
    expect(s.streamingIds).toEqual([]);
  });

  it("流式成功后 settle：temp 已被升级，settle 按真实 id 收尾不新增重复", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", content: "你好", audio_path: "" }));
    st.adoptStreaming(notice({ id: "m_real", content: "你好" }));
    st.settle("tmp1", msg({ id: "m_real", content: "你好" }));
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["m_real"]);
    expect(s.streamingIds).toEqual([]);
  });

  it("失败：fail 保留该行转失败态并记原因，从合成中摘除", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", content: "你好", audio_path: "" }));
    st.fail("tmp1", "连接超时");
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["tmp1"]); // 行保留可重试
    expect(s.failedIds).toContain("tmp1");
    expect(s.streamingIds).not.toContain("tmp1");
    expect(s.reasons["tmp1"]).toBe("连接超时");
  });

  it("失败：只对已存在的气泡生效，无行时不凭空补一条（避免游离失败气泡）", () => {
    useChatStore.getState().fail("m_ghost", "网络错误");
    const s = useChatStore.getState();
    expect(s.messages).toEqual([]);
    expect(s.failedIds).toEqual([]);
  });

  it("重试前 clearFailed：移除失败行与标记", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", content: "你好", audio_path: "" }));
    st.fail("tmp1", "网络错误");
    st.clearFailed("tmp1");
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual([]);
    expect(s.failedIds).toEqual([]);
  });

  it("mergeLatest：本地 temp / 失败气泡留在末尾，不被翻页合并挤到顶部", () => {
    const st = useChatStore.getState();
    st.load([msg({ id: "m_old", content: "很久以前" })], true);
    st.beginPlaceholder(msg({ id: "tmp1", content: "正在发", audio_path: "" }));
    // 模拟 message:changed 拉回最新一页（不含本地 temp）
    st.mergeLatest([msg({ id: "m_old", content: "很久以前" }), msg({ id: "m_persisted" })], true);
    const s = useChatStore.getState();
    expect(s.messages.map((m) => m.id)).toEqual(["m_old", "m_persisted", "tmp1"]);
    expect(s.streamingIds).toContain("tmp1");
  });

  it("mergeLatest：已落库的 id 从合成中摘除（跨窗口流式收尾）", () => {
    const st = useChatStore.getState();
    st.adoptStreaming(notice({ id: "m_real" }));
    st.mergeLatest([msg({ id: "m_real" })], false);
    const s = useChatStore.getState();
    expect(s.streamingIds).not.toContain("m_real");
    expect(s.messages.map((m) => m.id)).toEqual(["m_real"]);
  });

  it("removeMessage：删除时连同合成/失败标记一起清掉", () => {
    const st = useChatStore.getState();
    st.beginPlaceholder(msg({ id: "tmp1", audio_path: "" }));
    st.fail("tmp1", "x");
    st.removeMessage("tmp1");
    const s = useChatStore.getState();
    expect(s.messages).toEqual([]);
    expect(s.failedIds).toEqual([]);
    expect(s.streamingIds).toEqual([]);
  });
});
