// 消息列表 + 合成态（streaming / failed）的收敛状态机。
//
// 与 utils/streamPlaceholder.ts 的分工：那边是「无副作用的列表纯函数」（便于逐一单测），
// 这里把它们编排成 store action，让 App 的事件回调与发送流程读写同一份快照。
//
// 非阻塞发送的一条气泡会跨越三个异步来源：
// - 本窗 handleSend 抢先插的 temp 占位（阻塞引擎没有后端 early 事件，靠它做到「发送即进气泡」）；
// - 后端 tts:streaming 的真实 id（流式引擎会话建立、即将出声时发）；
// - generateTTS 返回值（成功）/ tts:failed 事件（流式中途失败，含跨窗口发送）。
// 用 store 集中管理，避免异步回调拿到过期 state 导致「temp 与真实 id 两条气泡并存」
// 「失败气泡被翻页合并挤到顶部」等竞态。

import { create } from "zustand";
import type { Message } from "../types";
import {
  appendPlaceholder,
  replacePlaceholder,
  withStreamingPlaceholder,
  markFailed,
  dropStreaming,
  withId,
  withoutId,
  type StreamingNotice,
} from "../utils/streamPlaceholder";

interface ChatState {
  messages: Message[];
  /** 正在合成中的消息 id（temp 占位或已收到 tts:streaming 但还没落库） */
  streamingIds: string[];
  /** 合成失败、保留在列表里等待重试的消息 id */
  failedIds: string[];
  /** 失败原因（按 id 存，渲染在失败气泡上） */
  reasons: Record<string, string>;
  /** 是否还有更早的消息可翻页 */
  hasMore: boolean;

  /** 首屏硬载：直接以服务端一页为准（丢弃任何遗留本地态） */
  load: (list: Message[], hasMore: boolean) => void;
  /** 单独更新“是否还有更早”（上滑翻页后用） */
  setHasMore: (hasMore: boolean) => void;
  /** 上滑翻页：把更早的一页接到前面（按 id 去重） */
  prependOlder: (list: Message[]) => void;
  /** 重取最新一页并合并：已落库的 id 从合成中摘除，本地 temp/失败占位保留在末尾 */
  mergeLatest: (list: Message[], hasMore: boolean) => void;
  /** 发送即抢先插一条 temp 占位（阻塞引擎 & 流式引擎统一先占位） */
  beginPlaceholder: (message: Message) => void;
  /** 收到 tts:streaming：把内容相同的 temp 占位原地升级为真实 id；无 temp（跨窗口）则追加 */
  adoptStreaming: (notice: StreamingNotice) => void;
  /** generateTTS 成功返回：用正式消息替换 temp 占位（或已升级的真实 id），清合成态 */
  settle: (tempId: string, message: Message) => void;
  /** 合成失败：把已存在的气泡转失败态并记原因；无对应气泡则忽略（不凭空补游离气泡） */
  fail: (id: string, reason: string) => void;
  /** 遥控/跨窗发送到达：按内容追加一条“合成中”占位（同内容已有则去重跳过） */
  remotePending: (content: string) => void;
  /** 跨窗口失败（如浮窗/遥控发送、连接前失败后端不发 tts:failed）：优先把同内容占位转失败，无占位才补一条 */
  remoteFailed: (content: string, reason: string) => void;
  /** 重试前清掉失败占位 */
  clearFailed: (id: string) => void;
  /** 删除某条消息（连同合成/失败标记一起清掉） */
  removeMessage: (id: string) => void;
}

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  streamingIds: [],
  failedIds: [],
  reasons: {},
  hasMore: false,

  load: (list, hasMore) =>
    set({ messages: list, streamingIds: [], failedIds: [], reasons: {}, hasMore }),

  setHasMore: (hasMore) => set({ hasMore }),

  prependOlder: (list) =>
    set((s) => {
      const existing = new Set(s.messages.map((m) => m.id));
      const older = list.filter((m) => !existing.has(m.id));
      return { messages: [...older, ...s.messages] };
    }),

  mergeLatest: (list, hasMore) =>
    set((s) => {
      const inWindow = new Set(list.map((m) => m.id));
      // 本次并入的真实（已落库）消息内容集合——用于收掉“同内容的合成中占位”。
      // 非流式引擎没有 tts:streaming 把占位升级成真实 id，只能靠内容把 temp 收敛掉，避免双气泡。
      const settledContents = new Set(
        list.filter((m) => m.audio_path !== "").map((m) => m.content),
      );
      const collapsedTempIds = new Set(
        s.messages
          .filter(
            (m) =>
              s.streamingIds.includes(m.id) &&
              m.audio_path === "" &&
              settledContents.has(m.content),
          )
          .map((m) => m.id),
      );
      // 本地未落库的占位：temp（audio_path 空）、合成中、失败——一律保留在末尾，
      // 不能被“更早消息在前 + 窗口在后”的合并挤到顶部。已在本次窗口里的 id 不算本地
      // （它已落库，交给 list 那一份，避免同一条被窗口与 locals 各加一次而重复）。
      // 例外：被真实消息按内容取代的合成中占位（collapsedTempIds）要丢弃，不再保留。
      const isLocal = (m: Message) =>
        m.audio_path === "" || s.streamingIds.includes(m.id) || s.failedIds.includes(m.id);
      const locals = s.messages.filter(
        (m) => isLocal(m) && !inWindow.has(m.id) && !collapsedTempIds.has(m.id),
      );
      const localIds = new Set(locals.map((m) => m.id));
      // 已落库的 id 从合成中摘除（跨窗口流式收尾）
      const settled = new Set(list.map((m) => m.id));
      const older = s.messages.filter(
        (m) => !inWindow.has(m.id) && !localIds.has(m.id) && !collapsedTempIds.has(m.id),
      );
      return {
        messages: [...older, ...list, ...locals],
        streamingIds: s.streamingIds.filter(
          (id) => !settled.has(id) && !collapsedTempIds.has(id),
        ),
        hasMore,
      };
    }),

  beginPlaceholder: (message) =>
    set((s) => ({
      messages: appendPlaceholder(s.messages, message),
      streamingIds: withId(s.streamingIds, message.id),
    })),

  adoptStreaming: (notice) =>
    set((s) => {
      // 找一条与本通知同文本、还没拿到音频的 temp 占位，按位置原地升级成真实 id
      const idx = s.messages.findIndex(
        (m) => m.id !== notice.id && m.audio_path === "" && m.content === notice.content,
      );
      if (idx >= 0) {
        const tempId = s.messages[idx].id;
        const next = s.messages.slice();
        next[idx] = {
          id: notice.id,
          content: notice.content,
          audio_path: notice.audio_path,
          created_at: notice.created_at,
        };
        return {
          messages: next,
          streamingIds: withId(withoutId(s.streamingIds, tempId), notice.id),
        };
      }
      // 没有可升级的 temp（跨窗口发送）：按 tts:streaming 追加占位
      return {
        messages: withStreamingPlaceholder(s.messages, notice),
        streamingIds: withId(s.streamingIds, notice.id),
      };
    }),

  settle: (tempId, message) =>
    set((s) => ({
      // temp 若已被 adoptStreaming 升级掉，replacePlaceholder 退化为按真实 id 替换
      messages: replacePlaceholder(s.messages, tempId, message),
      streamingIds: withoutId(withoutId(s.streamingIds, tempId), message.id),
    })),

  fail: (id, reason) =>
    set((s) => {
      if (!s.messages.some((m) => m.id === id)) return {}; // 无对应气泡：忽略
      const r = markFailed(s.messages, s.streamingIds, s.failedIds, {
        id,
        content: "",
        created_at: "",
      });
      return {
        messages: r.list,
        streamingIds: r.streamingIds,
        failedIds: r.failedIds,
        reasons: { ...s.reasons, [id]: reason },
      };
    }),

  remotePending: (content) =>
    set((s) => {
      const c = content.trim();
      if (!c) return {};
      // 去重：同内容已有“合成中/失败”占位（本窗发送、或已收到 send-started）就不再追加
      const dup = s.messages.some(
        (m) =>
          m.content === c &&
          (s.streamingIds.includes(m.id) || s.failedIds.includes(m.id)),
      );
      if (dup) return {};
      const id = `tmp_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
      return {
        messages: [
          ...s.messages,
          { id, content: c, audio_path: "", created_at: new Date().toISOString() },
        ],
        streamingIds: withId(s.streamingIds, id),
      };
    }),

  remoteFailed: (content, reason) =>
    set((s) => {
      const c = content.trim();
      // 1) 命中同内容的“合成中”占位 → 原地转失败（不新增、保持位置）
      const idx = s.messages.findIndex(
        (m) => m.content === c && s.streamingIds.includes(m.id),
      );
      if (idx >= 0) {
        const id = s.messages[idx].id;
        return {
          streamingIds: withoutId(s.streamingIds, id),
          failedIds: withId(s.failedIds, id),
          reasons: { ...s.reasons, [id]: reason || "" },
        };
      }
      // 2) 同内容已是失败态 → 不重复补
      if (s.messages.some((m) => m.content === c && s.failedIds.includes(m.id))) return {};
      // 3) 无占位（极少：send-started 丢失）→ 补一条失败气泡
      const id = `tmp_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
      return {
        messages: [
          ...s.messages,
          { id, content: c, audio_path: "", created_at: new Date().toISOString() },
        ],
        failedIds: withId(s.failedIds, id),
        reasons: { ...s.reasons, [id]: reason || "" },
      };
    }),

  clearFailed: (id) =>
    set((s) => {
      const r = dropStreaming(s.messages, id);
      return { messages: r, failedIds: withoutId(s.failedIds, id) };
    }),

  removeMessage: (id) =>
    set((s) => {
      const nextReasons = { ...s.reasons };
      delete nextReasons[id];
      return {
        messages: dropStreaming(s.messages, id),
        streamingIds: withoutId(s.streamingIds, id),
        failedIds: withoutId(s.failedIds, id),
        reasons: nextReasons,
      };
    }),
}));
