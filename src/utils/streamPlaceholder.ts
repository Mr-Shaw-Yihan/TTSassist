// 流式「占位消息」的列表状态机（纯函数，便于单测）。
//
// 为什么需要它：宿主流式合成是「先出声、后落库」——音频边合成边播，Message 记录要等
// 整条流跑完才写盘。若前端只在命令返回后入列，用户就会遇到「已经听到了声音，界面上
// 却没有这条消息，因而无处可点停止」。后端因此在会话建立、即将出声那一刻发
// `tts:streaming`（载荷字段与最终 Message 一致），前端据此先插一条占位消息；
// 命令返回后再用正式消息按同一 id 原地替换。
//
// 关键不变量：**同一条流式合成在列表里永远只占一格**，且替换不能把位置换到最后
// （否则消息会在播完那一刻跳位）。事件丢失/时序倒置也不能丢消息。

import type { Message } from "../types";

/** `tts:streaming` 事件载荷（与后端 StreamingNotice 逐字段对应） */
export interface StreamingNotice {
  id: string;
  content: string;
  audio_path: string;
  engine: string;
  created_at: string;
}

/** 收到「已开始流式」通知：把占位消息插到末尾；同 id 重复通知保持幂等。 */
export function withStreamingPlaceholder(
  list: Message[],
  notice: StreamingNotice,
): Message[] {
  if (list.some((m) => m.id === notice.id)) return list;
  return [
    ...list,
    {
      id: notice.id,
      content: notice.content,
      audio_path: notice.audio_path,
      created_at: notice.created_at,
    },
  ];
}

/**
 * 合成完成：用正式消息替换同 id 的占位（保持原位置）。
 * 列表里没有该 id（事件丢失、或通知还没送达就先返回了）时追加，绝不丢消息。
 */
export function settleStreaming(list: Message[], message: Message): Message[] {
  const idx = list.findIndex((m) => m.id === message.id);
  if (idx < 0) return [...list, message];
  const next = list.slice();
  next[idx] = message;
  return next;
}

/** 合成失败/未产出的占位回收：只按 id 摘除，不动其它条目。 */
export function dropStreaming(list: Message[], id: string): Message[] {
  return list.filter((m) => m.id !== id);
}

/** 某条消息是否仍是占位（正在合成中，音频文件尚未落盘完成） */
export function isStreamingPlaceholder(
  streamingIds: string[],
  message: Message,
): boolean {
  return streamingIds.includes(message.id);
}

// ── 非阻塞发送：阻塞引擎占位 / 失败态 / reconcile（供 App 的三份列表状态用）──

/** 集合幂等增删（streamingIds / failedIds 用），不改入参。 */
export function withId(ids: string[], id: string): string[] {
  return ids.includes(id) ? ids : [...ids, id];
}
export function withoutId(ids: string[], id: string): string[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : ids;
}

/** 某条是否已失败（渲染失败气泡 + 重试按钮）。 */
export function isFailed(failedIds: string[], message: Message): boolean {
  return failedIds.includes(message.id);
}

/** 发送即抢先插一条占位气泡（阻塞引擎没有后端 early 事件，前端用本地 tempId 先占位）。同 id 幂等。 */
export function appendPlaceholder(list: Message[], message: Message): Message[] {
  return list.some((m) => m.id === message.id) ? list : [...list, message];
}

/** 阻塞引擎：后端真实消息 id 与前端 tempId 不同，按位置把 temp 占位换成正式消息；
 *  找不到 temp 占位时退化为 settleStreaming 语义（按 id 替换/追加），绝不丢消息。 */
export function replacePlaceholder(list: Message[], tempId: string, message: Message): Message[] {
  const idx = list.findIndex((m) => m.id === tempId);
  if (idx < 0) return settleStreaming(list, message);
  const next = list.slice();
  next[idx] = message;
  return next;
}

/** 合成失败：把该 id 从 streaming 移到 failed；列表里若还没有这一行（阻塞且未抢先占位的意外情况）
 *  则补插一条占位行，保证“失败气泡保留可重试”。reason 文本由调用方另存。 */
export function markFailed(
  list: Message[],
  streamingIds: string[],
  failedIds: string[],
  entry: { id: string; content: string; created_at: string },
): { list: Message[]; streamingIds: string[]; failedIds: string[] } {
  const nextList = list.some((m) => m.id === entry.id)
    ? list
    : [...list, { ...entry, audio_path: "" }];
  return {
    list: nextList,
    streamingIds: withoutId(streamingIds, entry.id),
    failedIds: withId(failedIds, entry.id),
  };
}

/** 重试前清掉失败占位（同文本重发会另起一条 temp）。 */
export function clearFailed(
  list: Message[],
  failedIds: string[],
  id: string,
): { list: Message[]; failedIds: string[] } {
  return { list: dropStreaming(list, id), failedIds: withoutId(failedIds, id) };
}
